<?php
/**
 * The plugin inside a REAL WordPress, through WordPress's own REST dispatcher.
 * Run with the plugin active:  wp eval-file tests/integration-test.php
 * (src-tauri/examples/live_sync_plugin_check.rs does exactly that on a fixture site).
 *
 * Prints one ok/FAIL line per assertion and ends with "integration: N failed".
 *
 * @package rexenv-sync
 */

$fail  = 0;
$check = function ( $ok, $what ) use ( &$fail ) {
	echo ( $ok ? 'ok   ' : 'FAIL ' ) . $what . "\n";
	if ( ! $ok ) {
		$fail++;
	}
};

// A pairing, and the secret back out of its key — exactly what rexenv would hold.
$key    = Rexenv_Sync_Pairing::create();
$json   = json_decode( Rexenv_Sync_Signature::b64u_decode( substr( $key, strlen( 'rexsync1:' ) ) ), true );
$secret = Rexenv_Sync_Signature::b64u_decode( $json['s'] );
$check( 0 === strpos( $key, 'rexsync1:' ) && 32 === strlen( $secret ) && preg_match( '/^k_[0-9a-f]{8}$/', $json['k'] ), 'a pairing key in the §2 shape' );

$send = function ( $method, $route, array $query = array(), $body = '', array $override = array() ) use ( $json, $secret ) {
	$req = new WP_REST_Request( $method, $route );
	$pairs = array();
	foreach ( $query as $k => $v ) {
		$req->set_param( $k, $v );
		$pairs[] = array( $k, $v );
	}
	$req->set_query_params( $query );
	$req->set_body( $body );
	$ts    = isset( $override['ts'] ) ? $override['ts'] : (string) time();
	$nonce = isset( $override['nonce'] ) ? $override['nonce'] : Rexenv_Sync_Signature::b64u_encode( random_bytes( 16 ) );
	$sig   = Rexenv_Sync_Signature::sign( $secret, Rexenv_Sync_Signature::canonical( $method, $route, $pairs, $ts, $nonce, $body ) );
	if ( empty( $override['unsigned'] ) ) {
		$req->set_header( 'X-Rexsync-Key', isset( $override['key'] ) ? $override['key'] : $json['k'] );
		$req->set_header( 'X-Rexsync-Ts', $ts );
		$req->set_header( 'X-Rexsync-Nonce', $nonce );
		$req->set_header( 'X-Rexsync-Sig', $sig );
	}
	if ( isset( $override['body_after'] ) ) {
		$req->set_body( $override['body_after'] );
	}
	return rest_do_request( $req );
};
$code = function ( $resp ) {
	$d = $resp->get_data();
	return is_array( $d ) && isset( $d['code'] ) ? $d['code'] : null;
};

// 1. Every route refuses an unsigned request.
foreach ( Rexenv_Sync_Rest::routes() as $route => $spec ) {
	$r = $send( $spec[0], '/rexenv-sync/v1' . $route, array(), '', array( 'unsigned' => true ) );
	$check( 401 === $r->get_status() && 'unknown_key' === $code( $r ), "unsigned $spec[0] $route refused" );
}

// 2. The manifest.
$r = $send( 'GET', '/rexenv-sync/v1/manifest' );
$m = $r->get_data();
global $wpdb;
$names = array_column( isset( $m['tables'] ) ? $m['tables'] : array(), 'name' );
$check( 200 === $r->get_status() && 'rexsync1' === $m['protocol'] && untrailingslashit( home_url() ) === $m['site_url'] && in_array( $wpdb->options, $names, true ), 'signed manifest: protocol, site_url, the options table' );

// 3. Replay, skew, tamper, an old key.
$fixed = array( 'nonce' => 'AAECAwQFBgcICQoLDA0ODw' );
$first = $send( 'GET', '/rexenv-sync/v1/manifest', array(), '', $fixed );
$again = $send( 'GET', '/rexenv-sync/v1/manifest', array(), '', $fixed );
$check( 200 === $first->get_status() && 'replayed' === $code( $again ), 'the same nonce twice: the second is refused' );
$check( 'clock_skew' === $code( $send( 'GET', '/rexenv-sync/v1/manifest', array(), '', array( 'ts' => (string) ( time() - 400 ) ) ) ), 'a request 400 s old is refused' );
$check( 'bad_signature' === $code( $send( 'POST', '/rexenv-sync/v1/files/read', array(), '{"paths":[]}', array( 'body_after' => '{"paths":["x"]}' ) ) ), 'a body changed after signing is refused' );
$check( 'unknown_key' === $code( $send( 'GET', '/rexenv-sync/v1/manifest', array(), '', array( 'key' => 'k_ffffffff' ) ) ), 'another key id is refused' );

// 4. The file list: our own file is listed; an exclude removes it; the cursor resumes after it.
$own = 'plugins/rexenv-sync/rexenv-sync.php';
$all = array();
$cursor = null;
do {
	$q = null === $cursor ? array() : array( 'cursor' => $cursor );
	$d = $send( 'GET', '/rexenv-sync/v1/files/list', $q )->get_data();
	foreach ( $d['files'] as $f ) {
		$all[] = $f['path'];
	}
	$cursor = $d['cursor'];
} while ( null !== $cursor );
$check( in_array( $own, $all, true ), 'files/list includes the plugin\'s own main file' );
$check( count( $all ) === count( array_unique( $all ) ), 'files/list never repeats a path' );
$ex = $send( 'GET', '/rexenv-sync/v1/files/list', array( 'exclude' => 'plugins/*' ) )->get_data();
$check( ! in_array( $own, array_column( $ex['files'], 'path' ), true ), 'an exclude glob removes it' );
$after = $send( 'GET', '/rexenv-sync/v1/files/list', array( 'cursor' => $own ) )->get_data();
$paths = array_column( $after['files'], 'path' );
$check( ! in_array( $own, $paths, true ) && ( empty( $paths ) || strcmp( $paths[0], $own ) > 0 ), 'a cursor resumes strictly after itself' );

// 4b. The same list in SMALL pages (a 4 KB budget): every path exactly once, in the
//     same order, the walk stopping at each budget instead of walking everything.
add_filter( 'rexsync_max_list_bytes', function () { return 4096; } );
$paged  = array();
$pages  = 0;
$cursor = null;
do {
	$q = null === $cursor ? array() : array( 'cursor' => $cursor );
	$d = $send( 'GET', '/rexenv-sync/v1/files/list', $q )->get_data();
	foreach ( $d['files'] as $f ) {
		$paged[] = $f['path'];
	}
	$cursor = $d['cursor'];
	$pages++;
} while ( null !== $cursor && $pages < 10000 );
remove_all_filters( 'rexsync_max_list_bytes' );
$check( $pages > 3, "small pages: $pages of them" );
$check( $paged === $all, 'small pages list exactly the one-page list, in the same order' );

// 5. The file frame: the real file, and refusals for every path outside the rules.
$asked = array( $own, '../wp-config.php', '/etc/passwd', 'plugins/../../wp-config.php', 'uploads/rexsync-0000/x' );
$frame = Rexenv_Sync_Reader::file_frame( $asked, array() );
$recs  = array();
$at    = 0;
while ( true ) {
	$len = unpack( 'N', substr( $frame, $at, 4 ) )[1];
	$at += 4;
	if ( 0 === $len ) {
		break;
	}
	$h      = json_decode( substr( $frame, $at, $len ), true );
	$at    += $len;
	$bytes  = substr( $frame, $at, $h['size'] );
	$at    += $h['size'];
	$recs[] = array( $h, $bytes );
}
$check( count( $recs ) === count( $asked ) && strlen( $frame ) === $at, 'one record per path, then the terminator, nothing after it' );
$real = file_get_contents( WP_CONTENT_DIR . '/' . $own );
$check( $recs[0][1] === $real && hash( 'sha256', $real ) === $recs[0][0]['sha256'], 'the real file arrives whole, its sha256 right' );
$refused = true;
for ( $i = 1; $i < count( $recs ); $i++ ) {
	$refused = $refused && isset( $recs[ $i ][0]['error'] ) && 0 === $recs[ $i ][0]['size'];
}
$check( $refused, 'traversal, absolute and quarantine paths are refused, never read' );

// 6. The options table, exported: the site's rows, never the pairing.
$r = $send( 'GET', '/rexenv-sync/v1/db/export', array( 'table' => $wpdb->options ) );
$d = $r->get_data();
$check( 200 === $r->get_status() && false !== strpos( $d['sql'], "'siteurl'" ) && hash( 'sha256', $d['sql'] ) === $d['sha256'] && null === $d['cursor'], 'the options table exports, its sha256 right, in one chunk' );
$check( false === strpos( $d['sql'], 'rexsync_pairing' ) && false === strpos( $d['sql'], $json['s'] ), 'the export carries neither the pairing row nor the secret' );
$check( 0 === strpos( $d['sql'], 'DROP TABLE IF EXISTS' ), 'the first chunk starts with DROP + CREATE' );
$r = $send( 'GET', '/rexenv-sync/v1/db/export', array( 'table' => 'not_a_table' ) );
$check( 404 === $r->get_status(), 'an unknown table is refused' );

// 7. A table with BINARY cells, exported in several chunks across a delete: every
//    chunk's sha256 survives a JSON round trip, and keyset paging skips nothing.
$t    = $wpdb->prefix . 'rexsync_bin';
$copy = $wpdb->prefix . 'rexsync_bin_copy';
$wpdb->query( "DROP TABLE IF EXISTS `$t`, `$copy`" );
$wpdb->query( "CREATE TABLE `$t` (id INT NOT NULL AUTO_INCREMENT PRIMARY KEY, ip BINARY(4) NOT NULL, note TEXT) ENGINE=InnoDB" );
for ( $i = 0; $i < 450; $i++ ) {
	$wpdb->query( $wpdb->prepare( "INSERT INTO `$t` (ip, note) VALUES (%s, %s)", random_bytes( 4 ), "row $i; it's \"quoted\"\nand multi-line" ) );
}
$before = $wpdb->get_var( "SELECT COUNT(*) FROM `$t`" );
$want   = $wpdb->get_var( "SELECT SUM(CRC32(CONCAT(id, HEX(ip), note))) FROM `$t`" );
add_filter( 'rexsync_max_chunk_bytes', function () { return 2000; } );
$cursor = null;
$chunks = 0;
$sql    = '';
$intact = true;
do {
	$q = array( 'table' => $t );
	if ( null !== $cursor ) {
		$q['cursor'] = $cursor;
	}
	$data   = json_decode( wp_json_encode( $send( 'GET', '/rexenv-sync/v1/db/export', $q )->get_data() ), true ); // the wire
	$intact = $intact && hash( 'sha256', $data['sql'] ) === $data['sha256'];
	$sql   .= $data['sql'];
	$cursor = $data['cursor'];
	$chunks++;
	if ( 1 === $chunks ) {
		$wpdb->query( "DELETE FROM `$t` WHERE id = 1" ); // already exported: an offset would now skip a row
	}
} while ( null !== $cursor && $chunks < 1000 );
$check( $chunks > 2, "the export took several chunks ($chunks)" );
$check( $intact, 'every chunk\'s sha256 survives the JSON round trip, binary cells and all' );
foreach ( array_filter( explode( ";\n", str_replace( "`$t`", "`$copy`", $sql ) ) ) as $stmt ) {
	$wpdb->query( $stmt );
}
$got = $wpdb->get_var( "SELECT SUM(CRC32(CONCAT(id, HEX(ip), note))) FROM `$copy`" );
$check( (string) $want === (string) $got && (int) $before === (int) $wpdb->get_var( "SELECT COUNT(*) FROM `$copy`" ), 'the import of those chunks is the table, row for row, nothing skipped' );
$wpdb->query( "DROP TABLE IF EXISTS `$t`, `$copy`" );

Rexenv_Sync_Pairing::delete();
echo "integration: $fail failed\n";

<?php
/**
 * The plugin's signature against the SHARED vectors (docs/rexsync-protocol.md §7).
 * Plain PHP, no PHPUnit, so it runs on any PHP the developer has:
 *   php companion/rexenv-sync/tests/signature-test.php
 * Exit 0 = every vector agrees with the Rust client's; anything else names the one that did not.
 *
 * @package rexenv-sync
 */

require __DIR__ . '/../includes/class-rexenv-sync-signature.php';

$v      = json_decode( file_get_contents( __DIR__ . '/vectors.json' ), true );
$secret = Rexenv_Sync_Signature::b64u_decode( $v['secret_b64u'] );
$fail   = 0;
$check  = function ( $ok, $what ) use ( &$fail ) {
	echo ( $ok ? 'ok   ' : 'FAIL ' ) . $what . "\n";
	if ( ! $ok ) {
		$fail++;
	}
};

foreach ( $v['requests'] as $i => $r ) {
	$check( hash( 'sha256', $r['body'] ) === $r['body_sha256'], "vector $i body sha256" );
	$c = Rexenv_Sync_Signature::canonical( $r['method'], $r['path'], $r['query'], $v['ts'], $v['nonce'], $r['body'] );
	$check( $c === $r['canonical'], "vector $i canonical" );
	$check( Rexenv_Sync_Signature::sign( $secret, $c ) === $r['sig'], "vector $i signature" );

	// The check, in its order.
	$headers = array(
		'x-rexsync-key'   => $v['key_id'],
		'x-rexsync-ts'    => $v['ts'],
		'x-rexsync-nonce' => $v['nonce'],
		'x-rexsync-sig'   => $r['sig'],
	);
	$pairing = array( 'key_id' => $v['key_id'], 'secret' => $secret );
	$store   = array();
	$seen    = function ( $k ) use ( &$store ) {
		if ( isset( $store[ $k ] ) ) {
			return true;
		}
		$store[ $k ] = true;
		return false;
	};
	$now = (int) $v['ts'];
	$args = array( $headers, $r['method'], $r['path'], $r['query'], $r['body'], $pairing, $now );
	$check( null === Rexenv_Sync_Signature::check( ...array_merge( $args, array( $seen ) ) ), "vector $i accepted" );
	$check( 'replayed' === Rexenv_Sync_Signature::check( ...array_merge( $args, array( $seen ) ) ), "vector $i replay refused" );
	$skew = $args;
	$skew[6] = $now + 301;
	$check( 'clock_skew' === Rexenv_Sync_Signature::check( ...array_merge( $skew, array( $seen ) ) ), "vector $i skew refused" );
	$tamper = $args;
	$tamper[4] = $r['body'] . ' ';
	$check( 'bad_signature' === Rexenv_Sync_Signature::check( ...array_merge( $tamper, array( function () { return false; } ) ) ), "vector $i tampered body refused" );
	$other = $args;
	$other[5] = array( 'key_id' => 'k_ffffffff', 'secret' => $secret );
	$check( 'unknown_key' === Rexenv_Sync_Signature::check( ...array_merge( $other, array( $seen ) ) ), "vector $i old key refused" );
}

exit( $fail ? 1 : 0 );

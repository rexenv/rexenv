<?php
/**
 * What a pull reads (docs/rexsync-protocol.md §4.1): the manifest, the file list, the
 * file frame, and one table's SQL — each bounded per call, each resumable by cursor.
 *
 * Pure of HTTP: the REST layer (class-rexenv-sync-rest.php) only checks the signature
 * and hands arguments here, so tests can call these directly inside WordPress.
 *
 * @package rexenv-sync
 * @license GPL-2.0-or-later
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

final class Rexenv_Sync_Reader {

	/** Per-call budget — "≈ 8 MB or ≈ 15 s, whichever first" (§4). */
	const MAX_BYTES   = 8388608;
	const MAX_SECONDS = 15;

	/** Paths the plugin never lists or reads, whatever rexenv asks (§5). */
	const OWN_EXCLUDES = array( 'uploads/rexsync-*', 'uploads/rexsync-*/*', 'rexsync-backups/*' );

	public static function manifest() {
		global $wpdb;
		$tables = array();
		$like   = $wpdb->esc_like( $wpdb->base_prefix ) . '%';
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery
		$rows = $wpdb->get_results( $wpdb->prepare( 'SHOW TABLE STATUS LIKE %s', $like ), ARRAY_A );
		foreach ( (array) $rows as $r ) {
			if ( self::is_view( $r ) ) {
				continue; // a view is a query, not data: never listed, never exported
			}
			$tables[] = array(
				'name'     => $r['Name'],
				'rows'     => (int) $r['Rows'],
				'bytes'    => (int) $r['Data_length'] + (int) $r['Index_length'],
				'checksum' => self::checksum( $r ),
			);
		}
		$free = function_exists( 'disk_free_space' ) ? @disk_free_space( WP_CONTENT_DIR ) : false; // phpcs:ignore WordPress.PHP.NoSilencedErrors
		return array(
			'protocol'   => Rexenv_Sync_Signature::PROTOCOL,
			'plugin'     => REXENV_SYNC_VERSION,
			'site_url'   => untrailingslashit( home_url() ),
			'wp'         => get_bloginfo( 'version' ),
			'php'        => PHP_VERSION,
			'mysql'      => $wpdb->db_version(),
			'prefix'     => $wpdb->base_prefix,
			'multisite'  => is_multisite(),
			'charset'    => $wpdb->charset,
			'tables'     => $tables,
			'free_bytes' => false === $free ? null : (int) $free,
		);
	}

	/**
	 * The table's change stamp (§4.1): rows, data length and the engine's update
	 * time. NOT `CHECKSUM TABLE` — that reads the whole table on InnoDB, on every
	 * `/manifest`, and a big site's manifest ran past `max_execution_time` (review,
	 * 10 Oct 2026). rexenv only compares stamps of the same site for equality.
	 */
	private static function checksum( array $status ) {
		return 'rows:' . (int) $status['Rows'] . ':len:' . (int) $status['Data_length'] . ':upd:' . ( isset( $status['Update_time'] ) ? $status['Update_time'] : '' );
	}

	private static function is_view( array $status ) {
		return null === $status['Engine'] || ( isset( $status['Comment'] ) && 'VIEW' === strtoupper( (string) $status['Comment'] ) );
	}

	/** Is `rel` (relative to wp-content, `/`-separated) a path we may serve at all? */
	public static function safe_rel( $rel ) {
		if ( ! is_string( $rel ) || '' === $rel || '/' === $rel[0] || false !== strpos( $rel, '\\' ) || false !== strpos( $rel, "\0" ) || false !== strpos( $rel, ':' ) ) {
			return false;
		}
		foreach ( explode( '/', $rel ) as $seg ) {
			if ( '' === $seg || '.' === $seg || '..' === $seg ) {
				return false;
			}
		}
		return true;
	}

	/** Excluded by the plugin's own list or rexenv's `exclude` globs. */
	public static function excluded( $rel, array $client_globs ) {
		foreach ( array_merge( self::OWN_EXCLUDES, $client_globs ) as $glob ) {
			if ( fnmatch( $glob, $rel ) ) {
				return true;
			}
		}
		return false;
	}

	/**
	 * Files under wp-content in a stable order, resuming AFTER `cursor` (the last path
	 * the previous call returned), and STOPPING at the budget.
	 *
	 * The order is a depth-first walk with each directory's entries sorted by name —
	 * which is the lexicographic order of the paths' SEGMENT lists, so the cursor
	 * alone says where to resume: a directory the cursor is inside is descended, one
	 * that sorts before it is skipped whole, a file is listed only after it. The walk
	 * itself is bounded; the first version walked all of wp-content on every page
	 * and could pass `max_execution_time` on a big site (review, 10 Oct 2026).
	 */
	public static function list_files( $cursor, array $client_globs ) {
		$state = array(
			'out'    => array(),
			'bytes'  => 0,
			'start'  => microtime( true ),
			'cursor' => ( null === $cursor || '' === $cursor ) ? null : explode( '/', (string) $cursor ),
			'budget' => (int) apply_filters( 'rexsync_max_list_bytes', self::MAX_BYTES ),
			'last'   => null,
			'full'   => false,
		);
		self::walk( WP_CONTENT_DIR, array(), $client_globs, $state );
		return array( 'files' => $state['out'], 'cursor' => $state['full'] ? $state['last'] : null );
	}

	/** -1 / 0 / 1: segment-wise comparison of two paths. */
	private static function cmp_segments( array $a, array $b ) {
		$n = min( count( $a ), count( $b ) );
		for ( $i = 0; $i < $n; $i++ ) {
			$c = strcmp( $a[ $i ], $b[ $i ] );
			if ( 0 !== $c ) {
				return $c < 0 ? -1 : 1;
			}
		}
		return count( $a ) <=> count( $b );
	}

	private static function walk( $root, array $segs, array $globs, array &$state ) {
		$dir     = $root . ( $segs ? '/' . implode( '/', $segs ) : '' );
		$entries = @scandir( $dir ); // phpcs:ignore WordPress.PHP.NoSilencedErrors
		if ( false === $entries ) {
			return;
		}
		sort( $entries, SORT_STRING );
		foreach ( $entries as $e ) {
			if ( $state['full'] ) {
				return;
			}
			if ( '.' === $e || '..' === $e ) {
				continue;
			}
			$here = array_merge( $segs, array( $e ) );
			$rel  = implode( '/', $here );
			$full = $root . '/' . $rel;
			if ( is_link( $full ) || self::excluded( $rel, $globs ) ) {
				continue;
			}
			$c = $state['cursor'];
			if ( is_dir( $full ) ) {
				// Skip a directory that sorts wholly before the cursor; descend into one the
				// cursor is inside or that comes after it.
				$inside = null !== $c && count( $c ) > count( $here ) && array_slice( $c, 0, count( $here ) ) === $here;
				if ( null === $c || $inside || self::cmp_segments( $here, $c ) > 0 ) {
					self::walk( $root, $here, $globs, $state );
				}
				continue;
			}
			if ( ! is_file( $full ) || ( null !== $c && self::cmp_segments( $here, $c ) <= 0 ) ) {
				continue;
			}
			$state['out'][]  = array( 'path' => $rel, 'size' => (int) filesize( $full ), 'mtime' => (int) filemtime( $full ) );
			$state['last']   = $rel;
			$state['bytes'] += 64 + strlen( $rel );
			if ( $state['bytes'] > $state['budget'] || ( microtime( true ) - $state['start'] ) > self::MAX_SECONDS ) {
				$state['full'] = true;
			}
		}
	}

	/** The §4.3 frame for `paths` (≤ 200), as one string, terminator included. */
	public static function file_frame( array $paths, array $client_globs ) {
		$root = realpath( WP_CONTENT_DIR );
		$body = '';
		foreach ( array_slice( $paths, 0, 200 ) as $rel ) {
			$header = array( 'path' => (string) $rel );
			$full   = $root . '/' . $rel;
			$real   = self::safe_rel( $rel ) ? realpath( $full ) : false;
			if ( false === $real || 0 !== strpos( $real, $root . DIRECTORY_SEPARATOR ) || self::excluded( (string) $rel, $client_globs ) || ! is_file( $real ) ) {
				$header += array( 'size' => 0, 'mtime' => 0, 'sha256' => hash( 'sha256', '' ), 'error' => 'not a file this plugin serves' );
				$bytes   = '';
			} else {
				$bytes   = (string) file_get_contents( $real ); // phpcs:ignore WordPress.WP.AlternativeFunctions
				$header += array( 'size' => strlen( $bytes ), 'mtime' => (int) filemtime( $real ), 'sha256' => hash( 'sha256', $bytes ) );
			}
			$json  = wp_json_encode( $header, JSON_UNESCAPED_SLASHES );
			$body .= pack( 'N', strlen( $json ) ) . $json . $bytes;
		}
		return $body . pack( 'N', 0 );
	}

	/** Is `table` one of this site's tables (and therefore safe to name in SQL)? */
	private static function known_table( $table ) {
		foreach ( self::manifest_table_names() as $t ) {
			if ( $t === $table ) {
				return true;
			}
		}
		return false;
	}

	private static function manifest_table_names() {
		global $wpdb;
		$like = $wpdb->esc_like( $wpdb->base_prefix ) . '%';
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery
		// BASE TABLEs only — a view is never exported (it would arrive as a CREATE VIEW
		// followed by INSERTs into the view, which an import refuses).
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery
		return (array) $wpdb->get_col( $wpdb->prepare( "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' AND TABLE_NAME LIKE %s", $like ) );
	}

	/**
	 * One chunk of `table` as SQL (§4.1). The first chunk (cursor null) starts with
	 * DROP + CREATE; the cursor is the row offset. The plugin's own option rows are
	 * never in it — a pulled copy must not carry the live pairing.
	 */
	public static function export_table( $table, $cursor ) {
		global $wpdb;
		if ( ! self::known_table( $table ) ) {
			return new WP_Error( 'unknown_table', 'No such table on this site.', array( 'status' => 404 ) );
		}
		$q      = '`' . str_replace( '`', '``', $table ) . '`';
		$first  = null === $cursor || '' === $cursor;
		$offset = 0;
		$sql    = '';
		if ( $first ) {
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			$create = $wpdb->get_row( "SHOW CREATE TABLE $q", ARRAY_N );
			// The session the import runs in must accept what WordPress's own schema
			// holds: core tables default datetimes to '0000-00-00 00:00:00', which a
			// strict MySQL 8 refuses ("Invalid default value for 'comment_date'" — the
			// first whole pull, 10 Oct 2026). mysqldump writes the same header.
			$sql .= "/*!40101 SET SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;\n"
				. 'SET NAMES ' . ( $wpdb->charset ? esc_sql( $wpdb->charset ) : 'utf8mb4' ) . ";\n"
				. "SET FOREIGN_KEY_CHECKS=0;\n";
			$sql .= "DROP TABLE IF EXISTS $q;\n" . $create[1] . ";\n";
		}
		$where = '';
		if ( $table === $wpdb->options || ( is_multisite() && $table === $wpdb->sitemeta ) ) {
			$col   = $table === $wpdb->options ? 'option_name' : 'meta_key';
			$parts = array();
			foreach ( Rexenv_Sync_Pairing::owned_option_patterns() as $pat ) {
				$parts[] = $wpdb->prepare( "`$col` NOT LIKE %s", $pat ); // phpcs:ignore WordPress.DB.PreparedSQL.InterpolatedNotPrepared
			}
			$where = ' WHERE ' . implode( ' AND ', $parts );
		}
		// Paging (review, 10 Oct 2026): with a single-column primary key, by KEY —
		// `WHERE pk > last` — so a row deleted between two calls cannot shift the
		// next page and skip one. Otherwise by offset in primary-key order (composite
		// keys), or by offset alone (no key) — the residual risk the plan's conflict
		// pass exists for. The cursor says which: "k:<json value>" or "o:<offset>".
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
		$keys  = $wpdb->get_col( "SHOW KEYS FROM $q WHERE Key_name = 'PRIMARY'", 4 );
		$by_key = 1 === count( $keys );
		$order  = $keys ? ' ORDER BY `' . implode( '`,`', array_map( 'esc_sql', $keys ) ) . '`' : '';
		$last   = null;
		if ( is_string( $cursor ) && 0 === strpos( $cursor, 'k:' ) ) {
			$last = json_decode( substr( $cursor, 2 ), true );
		} elseif ( is_string( $cursor ) && 0 === strpos( $cursor, 'o:' ) ) {
			$offset = max( 0, (int) substr( $cursor, 2 ) );
		}
		$start = microtime( true );
		$rows  = 0;
		$batch = 200;
		while ( true ) {
			$cond = $where;
			if ( $by_key && null !== $last ) {
				$cond .= ( '' === $where ? ' WHERE ' : ' AND ' ) . $wpdb->prepare( '`' . esc_sql( $keys[0] ) . '` > %s', $last ); // phpcs:ignore WordPress.DB.PreparedSQL.NotPrepared
			}
			$limit = $by_key ? " LIMIT $batch" : " LIMIT $offset, $batch";
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			$chunk = $wpdb->get_results( "SELECT * FROM $q$cond$order$limit", ARRAY_A );
			if ( ! $chunk ) {
				return array( 'table' => $table, 'sql' => $sql, 'sha256' => hash( 'sha256', $sql ), 'rows' => $rows, 'cursor' => null );
			}
			$values = array();
			foreach ( $chunk as $row ) {
				$cells = array();
				foreach ( $row as $cell ) {
					$cells[] = self::sql_value( $cell );
				}
				$values[] = '(' . implode( ',', $cells ) . ')';
			}
			$cols    = '`' . implode( '`,`', array_map( 'esc_sql', array_keys( $chunk[0] ) ) ) . '`';
			$sql    .= "INSERT INTO $q ($cols) VALUES\n" . implode( ",\n", $values ) . ";\n";
			$rows   += count( $chunk );
			$offset += count( $chunk );
			if ( $by_key ) {
				$last = $chunk[ count( $chunk ) - 1 ][ $keys[0] ];
			}
			$next = $by_key ? 'k:' . wp_json_encode( $last ) : 'o:' . $offset;
			if ( count( $chunk ) < $batch ) {
				return array( 'table' => $table, 'sql' => $sql, 'sha256' => hash( 'sha256', $sql ), 'rows' => $rows, 'cursor' => null );
			}
			if ( strlen( $sql ) > (int) apply_filters( 'rexsync_max_chunk_bytes', self::MAX_BYTES ) || ( microtime( true ) - $start ) > self::MAX_SECONDS ) {
				return array( 'table' => $table, 'sql' => $sql, 'sha256' => hash( 'sha256', $sql ), 'rows' => $rows, 'cursor' => $next );
			}
		}
	}

	/**
	 * One cell as a SQL literal. A value that is not valid UTF-8 (a `binary(16)` IP in
	 * Wordfence's tables, a serialized blob with raw bytes) is written as `0x<hex>`:
	 * the chunk travels inside JSON, and JSON encoding rewrites invalid UTF-8 — the
	 * client then saw text that no longer matched its sha256, and a pull of any such
	 * table failed every time (review, 10 Oct 2026).
	 */
	public static function sql_value( $cell ) {
		global $wpdb;
		if ( null === $cell ) {
			return 'NULL';
		}
		$cell = (string) $cell;
		if ( '' !== $cell && ! preg_match( '//u', $cell ) ) {
			return '0x' . bin2hex( $cell );
		}
		return "'" . $wpdb->_real_escape( $cell ) . "'";
	}
}

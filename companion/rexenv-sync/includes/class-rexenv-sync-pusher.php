<?php
/**
 * What a push writes (docs/rexsync-protocol.md §4.2) — and the one promise that
 * matters: LIVE IS NEVER WRITTEN TO EXCEPT BY `swap` AND `rollback`, and `swap`
 * never runs without the backup it makes in the same request.
 *
 * Everything before the swap lands in two places nobody serves: a quarantine
 * folder under uploads (random name, index.php, .htaccess deny) and shadow
 * tables `<prefix>rxnew_<table>`. The swap is one `RENAME TABLE` for every
 * table at once (atomic in MySQL), then files moved with the overwritten ones
 * kept under a backup folder. One backup is kept — the last push.
 *
 * @package rexenv-sync
 * @license GPL-2.0-or-later
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

final class Rexenv_Sync_Pusher {

	const SHADOW  = 'rxnew_';
	const BACKUP  = 'rxbak_';
	const OPTION  = 'rexsync_push_';  // + push_id: the push's own record (tables, files, state)
	const KEEP    = 'rexsync_backup'; // the ONE backup kept: {id, tables, files, at}

	/** The option rows this plugin owns — restored after every swap and rollback. */
	private static function own_rows() {
		return array( Rexenv_Sync_Pairing::OPTION, self::KEEP );
	}

	private static function quarantine( $push_id ) {
		return WP_CONTENT_DIR . '/uploads/rexsync-' . $push_id;
	}

	private static function backup_dir( $id ) {
		return WP_CONTENT_DIR . '/rexsync-backups/' . $id;
	}

	private static function push_record( $push_id ) {
		$r = get_option( self::OPTION . $push_id );
		return is_array( $r ) ? $r : null;
	}

	private static function save_record( $push_id, array $r ) {
		update_option( self::OPTION . $push_id, $r, false );
	}

	/** A table the push may name: one of this site's own base tables. */
	private static function site_tables() {
		global $wpdb;
		$like = $wpdb->esc_like( $wpdb->base_prefix ) . '%';
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery
		$names = (array) $wpdb->get_col( $wpdb->prepare( "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_TYPE = 'BASE TABLE' AND TABLE_NAME LIKE %s", $like ) );
		return array_values( array_filter( $names, function ( $n ) { return ! Rexenv_Sync_Reader::is_own_table( $n ); } ) );
	}

	/**
	 * §4.2 `begin`: compare `base` (what rexenv last saw) with now; refuse with the
	 * conflicts unless each is in `override`; otherwise open the quarantine and
	 * the push's record. Returns the response array, or a WP_Error.
	 *
	 * @param array $tables   Table names the push will replace.
	 * @param array $files    wp-content-relative paths the push will write.
	 * @param array $base     ['tables' => [name => stamp], 'files' => [path => "size:mtime"]].
	 * @param array $override Items the user chose to overwrite despite a conflict.
	 */
	public static function begin( array $tables, array $files, array $base, array $override ) {
		$known = self::site_tables();
		foreach ( $tables as $t ) {
			if ( ! in_array( $t, $known, true ) ) {
				return new WP_Error( 'unknown_table', "No such table on this site: $t", array( 'status' => 404 ) );
			}
		}
		foreach ( $files as $f ) {
			if ( ! Rexenv_Sync_Reader::safe_rel( $f ) ) {
				return new WP_Error( 'bad_path', "Not a path this plugin writes: $f", array( 'status' => 400 ) );
			}
		}
		$conflicts = self::conflicts( $tables, $files, $base );
		$blocking  = array_values( array_diff( $conflicts, $override ) );
		if ( $blocking ) {
			return new WP_Error( 'conflict', 'The live site changed since rexenv last saw it.', array( 'status' => 409, 'conflicts' => $conflicts ) );
		}
		$push_id = bin2hex( random_bytes( 16 ) );
		$dir     = self::quarantine( $push_id );
		if ( ! wp_mkdir_p( $dir ) ) {
			return new WP_Error( 'quota', 'Could not make the upload folder for the push.', array( 'status' => 507 ) );
		}
		file_put_contents( $dir . '/index.php', "<?php // rexenv Sync quarantine\n" );
		file_put_contents( $dir . '/.htaccess', "Deny from all\n" );
		self::save_record( $push_id, array( 'tables' => array_values( $tables ), 'files' => array_values( $files ), 'db_done' => array(), 'state' => 'open', 'at' => time() ) );
		return array( 'push_id' => $push_id, 'conflicts' => $conflicts );
	}

	/** Tables whose stamp, and files whose size:mtime, differ from `base`. */
	public static function conflicts( array $tables, array $files, array $base ) {
		global $wpdb;
		$out = array();
		$now = array();
		foreach ( Rexenv_Sync_Reader::table_status() as $r ) {
			$now[ $r['Name'] ] = Rexenv_Sync_Reader::stamp( $r );
		}
		$base_t = isset( $base['tables'] ) && is_array( $base['tables'] ) ? $base['tables'] : array();
		foreach ( $tables as $t ) {
			if ( isset( $base_t[ $t ] ) && isset( $now[ $t ] ) && (string) $base_t[ $t ] !== (string) $now[ $t ] ) {
				$out[] = $t;
			}
		}
		$base_f = isset( $base['files'] ) && is_array( $base['files'] ) ? $base['files'] : array();
		foreach ( $files as $f ) {
			$full = WP_CONTENT_DIR . '/' . $f;
			if ( ! isset( $base_f[ $f ] ) || ! is_file( $full ) ) {
				continue;
			}
			if ( (string) $base_f[ $f ] !== filesize( $full ) . ':' . filemtime( $full ) ) {
				$out[] = $f;
			}
		}
		return $out;
	}

	/** §4.2 `file`: raw bytes into the quarantine, appended at `offset`. */
	public static function receive_file( $push_id, $rel, $offset, $bytes ) {
		$r = self::push_record( $push_id );
		if ( ! $r || 'open' !== $r['state'] ) {
			return new WP_Error( 'no_push', 'No open push with that id.', array( 'status' => 404 ) );
		}
		if ( ! in_array( $rel, $r['files'], true ) ) {
			return new WP_Error( 'bad_path', "$rel was not named when the push began.", array( 'status' => 400 ) );
		}
		$full = self::quarantine( $push_id ) . '/' . $rel;
		wp_mkdir_p( dirname( $full ) );
		$offset = (int) $offset;
		if ( 0 === $offset ) {
			file_put_contents( $full, $bytes );
		} else {
			if ( ! is_file( $full ) || filesize( $full ) !== $offset ) {
				return new WP_Error( 'bad_offset', "$rel: the file in the quarantine is not $offset bytes long.", array( 'status' => 409 ) );
			}
			file_put_contents( $full, $bytes, FILE_APPEND );
		}
		return array( 'received' => filesize( $full ) );
	}

	/**
	 * §4.2 `db`: one chunk of SQL for `table`, run against the SHADOW table. Every
	 * backtick-quoted table name must be `table` itself; it is rewritten to the
	 * shadow name, and a statement naming any other table of this site is refused.
	 */
	public static function receive_sql( $push_id, $table, $sql, $sha256 ) {
		global $wpdb;
		$r = self::push_record( $push_id );
		if ( ! $r || 'open' !== $r['state'] ) {
			return new WP_Error( 'no_push', 'No open push with that id.', array( 'status' => 404 ) );
		}
		if ( ! in_array( $table, $r['tables'], true ) ) {
			return new WP_Error( 'unknown_table', "$table was not named when the push began.", array( 'status' => 400 ) );
		}
		if ( hash( 'sha256', $sql ) !== $sha256 ) {
			return new WP_Error( 'bad_chunk', 'The chunk does not match its checksum.', array( 'status' => 400 ) );
		}
		$shadow = self::SHADOW . $table;
		foreach ( self::site_tables() as $other ) {
			if ( $other !== $table && false !== strpos( $sql, '`' . $other . '`' ) ) {
				return new WP_Error( 'bad_chunk', "A chunk for $table names another table ($other).", array( 'status' => 400 ) );
			}
		}
		$sql = str_replace( '`' . $table . '`', '`' . $shadow . '`', $sql );
		foreach ( self::statements( $sql ) as $stmt ) {
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			if ( false === $wpdb->query( $stmt ) ) {
				return new WP_Error( 'sql', "The site's database refused a statement for $table: " . $wpdb->last_error, array( 'status' => 500 ) );
			}
		}
		$r['db_done'][ $table ] = true;
		self::save_record( $push_id, $r );
		return array( 'ok' => true );
	}

	/**
	 * Split a dump on statement ends. Dumps escape newlines inside strings as `\n`,
	 * so `;\n` never occurs in a value. mysqldump puts `-- ` comment lines BEFORE a
	 * statement in the same piece (`-- Table structure…` then `DROP TABLE`), so
	 * comment LINES are stripped, never the piece.
	 */
	private static function statements( $sql ) {
		$out = array();
		foreach ( explode( ";\n", $sql ) as $part ) {
			$lines = array();
			foreach ( explode( "\n", $part ) as $line ) {
				if ( 0 !== strpos( ltrim( $line ), '--' ) ) {
					$lines[] = $line;
				}
			}
			$part = trim( implode( "\n", $lines ) );
			if ( '' !== $part ) {
				$out[] = $part;
			}
		}
		return $out;
	}

	/**
	 * §4.2 `swap`: the backup and the swap in ONE request — maintenance on, the
	 * previous backup dropped, every table renamed in one statement (live →
	 * backup, shadow → live), files moved with the overwritten ones kept, the
	 * plugin's own rows put back, caches flushed, maintenance off.
	 */
	public static function swap( $push_id ) {
		global $wpdb;
		$r = self::push_record( $push_id );
		if ( ! $r || 'open' !== $r['state'] ) {
			return new WP_Error( 'no_push', 'No open push with that id.', array( 'status' => 404 ) );
		}
		foreach ( $r['tables'] as $t ) {
			if ( empty( $r['db_done'][ $t ] ) ) {
				return new WP_Error( 'incomplete', "The push has no data for $t yet.", array( 'status' => 409 ) );
			}
		}
		$own = array();
		foreach ( self::own_rows() as $name ) {
			$own[ $name ] = get_option( $name );
		}
		self::drop_backup(); // one backup kept: the previous one goes before the new one is made
		self::maintenance( true );
		$pairs = array();
		foreach ( $r['tables'] as $t ) {
			$pairs[] = sprintf( '`%1$s` TO `%2$s`, `%3$s` TO `%1$s`', $t, self::BACKUP . $t, self::SHADOW . $t );
		}
		if ( $pairs ) {
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			if ( false === $wpdb->query( 'RENAME TABLE ' . implode( ', ', $pairs ) ) ) {
				self::maintenance( false );
				return new WP_Error( 'sql', 'The swap failed, nothing was changed: ' . $wpdb->last_error, array( 'status' => 500 ) );
			}
		}
		$bdir  = self::backup_dir( $push_id );
		$moved = array();
		foreach ( $r['files'] as $rel ) {
			$from = self::quarantine( $push_id ) . '/' . $rel;
			$to   = WP_CONTENT_DIR . '/' . $rel;
			if ( ! is_file( $from ) ) {
				continue; // named but never sent: left as is
			}
			if ( is_file( $to ) ) {
				wp_mkdir_p( dirname( $bdir . '/' . $rel ) );
				rename( $to, $bdir . '/' . $rel );
			} else {
				wp_mkdir_p( dirname( $to ) );
			}
			rename( $from, $to );
			$moved[] = $rel;
		}
		// The cache FIRST: the options table under WordPress just changed, and
		// `get_option` would otherwise answer from the old table's cached rows — the
		// first whole push kept the plugin "active" in cache only (10 Oct 2026).
		wp_cache_flush();
		foreach ( $own as $name => $value ) {
			if ( false !== $value ) {
				update_option( $name, $value, false );
			}
		}
		self::ensure_active();
		update_option( self::KEEP, array( 'id' => $push_id, 'tables' => $r['tables'], 'files' => $moved, 'at' => time() ), false );
		$r['state'] = 'swapped';
		self::save_record( $push_id, $r );
		self::remove_dir( self::quarantine( $push_id ) );
		delete_option( 'rewrite_rules' );
		self::maintenance( false );
		return array( 'swapped' => $r['tables'], 'files' => count( $moved ), 'backup_id' => $push_id );
	}

	/** §4.2 `rollback`: the kept backup back in place — tables renamed back, files restored. */
	public static function rollback( $backup_id ) {
		global $wpdb;
		$keep = get_option( self::KEEP );
		if ( ! is_array( $keep ) || $keep['id'] !== $backup_id ) {
			return new WP_Error( 'no_backup', 'That backup is not the one this site keeps.', array( 'status' => 404 ) );
		}
		$own = array();
		foreach ( self::own_rows() as $name ) {
			$own[ $name ] = get_option( $name );
		}
		self::maintenance( true );
		$pairs = array();
		foreach ( $keep['tables'] as $t ) {
			$pairs[] = sprintf( '`%1$s` TO `%2$s`, `%3$s` TO `%1$s`', $t, self::SHADOW . $t, self::BACKUP . $t );
		}
		if ( $pairs ) {
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			if ( false === $wpdb->query( 'RENAME TABLE ' . implode( ', ', $pairs ) ) ) {
				self::maintenance( false );
				return new WP_Error( 'sql', 'The rollback failed, nothing was changed: ' . $wpdb->last_error, array( 'status' => 500 ) );
			}
			foreach ( $keep['tables'] as $t ) {
				// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
				$wpdb->query( 'DROP TABLE IF EXISTS `' . self::SHADOW . $t . '`' );
			}
		}
		$bdir = self::backup_dir( $backup_id );
		foreach ( $keep['files'] as $rel ) {
			$pushed = WP_CONTENT_DIR . '/' . $rel;
			$old    = $bdir . '/' . $rel;
			if ( is_file( $old ) ) {
				rename( $old, $pushed );
			} elseif ( is_file( $pushed ) ) {
				unlink( $pushed ); // the push ADDED it; before it there was nothing
			}
		}
		wp_cache_flush(); // before any read — see `swap`
		foreach ( $own as $name => $value ) {
			if ( false !== $value ) {
				update_option( $name, $value, false );
			}
		}
		self::ensure_active();
		delete_option( self::KEEP );
		self::remove_dir( $bdir );
		delete_option( 'rewrite_rules' );
		self::maintenance( false );
		return array( 'restored' => $keep['tables'], 'files' => count( $keep['files'] ) );
	}

	/** §4.2 `abort`: an open push's shadow tables and quarantine, gone. */
	public static function abort( $push_id ) {
		global $wpdb;
		$r = self::push_record( $push_id );
		if ( $r ) {
			foreach ( $r['tables'] as $t ) {
				// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
				$wpdb->query( 'DROP TABLE IF EXISTS `' . self::SHADOW . $t . '`' );
			}
			delete_option( self::OPTION . $push_id );
		}
		self::remove_dir( self::quarantine( $push_id ) );
		return array( 'removed' => true );
	}

	/**
	 * Keep THIS plugin active across a swap. The pushed options table came from
	 * the local copy, where rexenv deactivates the plugin (plan §2.8) — so the
	 * first whole push took the plugin's own routes away with it (10 Oct 2026).
	 */
	public static function ensure_active() {
		$me     = 'rexenv-sync/rexenv-sync.php';
		$active = get_option( 'active_plugins' );
		$active = is_array( $active ) ? $active : array();
		if ( ! in_array( $me, $active, true ) ) {
			$active[] = $me;
			update_option( 'active_plugins', $active );
		}
	}

	/** The previous backup's tables and folder. */
	private static function drop_backup() {
		global $wpdb;
		$keep = get_option( self::KEEP );
		if ( is_array( $keep ) ) {
			foreach ( $keep['tables'] as $t ) {
				// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
				$wpdb->query( 'DROP TABLE IF EXISTS `' . self::BACKUP . $t . '`' );
			}
			self::remove_dir( self::backup_dir( $keep['id'] ) );
			delete_option( self::KEEP );
		}
	}

	/** WordPress's own maintenance switch: the `.maintenance` file beside wp-config. */
	private static function maintenance( $on ) {
		$file = ABSPATH . '.maintenance';
		if ( $on ) {
			file_put_contents( $file, '<?php $upgrading = ' . time() . ";\n" );
		} elseif ( is_file( $file ) ) {
			unlink( $file );
		}
	}

	/** rm -r, ours only: the quarantine and backup folders this class makes. */
	private static function remove_dir( $dir ) {
		if ( ! is_dir( $dir ) || 0 !== strpos( $dir, WP_CONTENT_DIR . '/' ) ) {
			return;
		}
		foreach ( scandir( $dir ) as $e ) {
			if ( '.' === $e || '..' === $e ) {
				continue;
			}
			$p = $dir . '/' . $e;
			is_dir( $p ) ? self::remove_dir( $p ) : unlink( $p );
		}
		rmdir( $dir );
	}
}

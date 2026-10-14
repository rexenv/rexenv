<?php
/**
 * The one pairing this site holds (docs/rexsync-protocol.md §2).
 *
 * Stored in its own option row, autoload off, and excluded from every export and
 * preserved across every push — a pulled copy must never carry the live secret.
 *
 * @package rexenv-sync
 * @license GPL-2.0-or-later
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

final class Rexenv_Sync_Pairing {

	const OPTION = 'rexsync_pairing';

	/** Every option row this plugin owns — never exported, never overwritten by a push. */
	public static function owned_option_patterns() {
		// The push's records and the kept backup are the LIVE site's state: a pulled
		// copy that carried `rexsync_backup` would believe it had a backup (#845).
		return array( self::OPTION, 'rexsync\_push\_%', 'rexsync_backup', self::LAST, '_transient_rexsync_%', '_transient_timeout_rexsync_%' );
	}

	/**
	 * What rexenv last did here (`{seen: {at, route}, pull: {at, table}, push: {at,
	 * id, tables, files}, rollback: {at, id}}`) — the admin page's "last contact"
	 * and "last pull/push" lines. The plugin's own row: never exported, left out of
	 * the options stamp (`Rexenv_Sync_Reader::VOLATILE_OPTIONS`), autoload off.
	 */
	const LAST = 'rexsync_last';

	/** Record `$what` ("seen" | "pull" | "push" | "rollback") at now, with `$extra`. */
	public static function note( $what, array $extra = array() ) {
		$row = get_option( self::LAST );
		if ( ! is_array( $row ) ) {
			$row = array();
		}
		$now = time();
		// "seen" is every signed request: one write a minute is plenty for a line
		// that says "a minute ago" (a pull is ~25 requests).
		if ( 'seen' === $what && isset( $row['seen']['at'] ) && $now - (int) $row['seen']['at'] < 60 ) {
			return;
		}
		$row[ $what ] = array_merge( array( 'at' => $now ), $extra );
		update_option( self::LAST, $row, false );
	}

	/** The record, or an empty array. */
	public static function last() {
		$row = get_option( self::LAST );
		return is_array( $row ) ? $row : array();
	}

	/** ['key_id' => ..., 'secret' => raw bytes] or an empty array. */
	public static function get() {
		$row = get_option( self::OPTION );
		if ( ! is_array( $row ) || empty( $row['key_id'] ) || empty( $row['secret'] ) ) {
			return array();
		}
		$secret = Rexenv_Sync_Signature::b64u_decode( $row['secret'] );
		if ( false === $secret || 32 !== strlen( $secret ) ) {
			return array();
		}
		return array( 'key_id' => $row['key_id'], 'secret' => $secret );
	}

	/**
	 * Make a NEW pairing — replacing any old one, which then stops working on its
	 * next request — and return the key to show ONCE.
	 */
	public static function create() {
		$secret = random_bytes( 32 );
		$key_id = 'k_' . bin2hex( random_bytes( 4 ) );
		update_option(
			self::OPTION,
			array(
				'key_id'  => $key_id,
				'secret'  => Rexenv_Sync_Signature::b64u_encode( $secret ),
				'created' => time(),
			),
			false
		);
		return self::key_text( untrailingslashit( home_url() ), $key_id, $secret );
	}

	/** The pasteable key (§2): rexsync1:<base64url(JSON)>. */
	public static function key_text( $site_url, $key_id, $secret ) {
		$json = wp_json_encode(
			array(
				'u' => $site_url,
				'k' => $key_id,
				's' => Rexenv_Sync_Signature::b64u_encode( $secret ),
			),
			JSON_UNESCAPED_SLASHES
		);
		return Rexenv_Sync_Signature::PROTOCOL . ':' . Rexenv_Sync_Signature::b64u_encode( $json );
	}

	public static function delete() {
		delete_option( self::OPTION );
		delete_option( self::LAST );
	}

	/** The nonce table: `<prefix>rexsync_nonces` — its OWN table, never in options. */
	public static function nonce_table() {
		global $wpdb;
		return $wpdb->prefix . 'rexsync_nonces';
	}

	/** Make the nonce table (activation, and lazily if it is missing). */
	public static function ensure_nonce_table() {
		global $wpdb;
		$t = self::nonce_table();
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
		$wpdb->query( "CREATE TABLE IF NOT EXISTS `$t` (k CHAR(40) NOT NULL PRIMARY KEY, at INT UNSIGNED NOT NULL) ENGINE=InnoDB" );
	}

	/**
	 * Has this nonce been seen in the last NONCE_TTL? Records it when not — in ONE
	 * step: an INSERT on a primary key, so of two copies of a request arriving
	 * together exactly one wins (a get-then-set let both pass; review, 10 Oct 2026).
	 *
	 * In its own table, NOT `wp_options` (10 Oct 2026, the first whole push): a
	 * nonce row per signed request moved the options table's stamp on every call,
	 * so every push saw `wp_options` as "changed on live since the base".
	 */
	public static function nonce_seen( $nonce_key ) {
		global $wpdb;
		$t   = self::nonce_table();
		$k   = substr( $nonce_key, 0, 40 );
		$now = time();
		$suppress = $wpdb->suppress_errors( true );
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
		$n = $wpdb->query( $wpdb->prepare( "INSERT IGNORE INTO `$t` (k, at) VALUES (%s, %d)", $k, $now ) );
		if ( false === $n ) {
			self::ensure_nonce_table();
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			$n = $wpdb->query( $wpdb->prepare( "INSERT IGNORE INTO `$t` (k, at) VALUES (%s, %d)", $k, $now ) );
		}
		$wpdb->suppress_errors( $suppress );
		if ( 1 === (int) $n ) {
			if ( 0 === wp_rand( 0, 49 ) ) {
				// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
				$wpdb->query( $wpdb->prepare( "DELETE FROM `$t` WHERE at < %d", $now - Rexenv_Sync_Signature::NONCE_TTL ) );
			}
			return false;
		}
		// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
		$at = (int) $wpdb->get_var( $wpdb->prepare( "SELECT at FROM `$t` WHERE k = %s", $k ) );
		if ( $now - $at > Rexenv_Sync_Signature::NONCE_TTL ) {
			// phpcs:ignore WordPress.DB.DirectDatabaseQuery, WordPress.DB.PreparedSQL.NotPrepared
			$wpdb->query( $wpdb->prepare( "UPDATE `$t` SET at = %d WHERE k = %s", $now, $k ) ); // an expired nonce, used again much later
			return false;
		}
		return true;
	}
}

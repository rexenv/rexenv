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
		return array( self::OPTION, '_transient_rexsync_%', '_transient_timeout_rexsync_%' );
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
	}

	/** Has this nonce been seen in the last NONCE_TTL? Records it when not. */
	public static function nonce_seen( $nonce_key ) {
		$t = 'rexsync_n_' . substr( $nonce_key, 0, 40 );
		if ( false !== get_transient( $t ) ) {
			return true;
		}
		set_transient( $t, 1, Rexenv_Sync_Signature::NONCE_TTL );
		return false;
	}
}

<?php
/**
 * Tools → rexenv Sync: connect, show the key ONCE, regenerate, disconnect.
 *
 * @package rexenv-sync
 * @license GPL-2.0-or-later
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

final class Rexenv_Sync_Admin {

	public static function menu() {
		add_management_page( 'rexenv Sync', 'rexenv Sync', 'manage_options', 'rexenv-sync', array( __CLASS__, 'page' ) );
	}

	public static function page() {
		if ( ! current_user_can( 'manage_options' ) ) {
			return;
		}
		$key = null;
		if ( isset( $_POST['rexsync_action'] ) && check_admin_referer( 'rexsync' ) ) {
			$action = sanitize_key( wp_unslash( $_POST['rexsync_action'] ) );
			if ( 'connect' === $action ) {
				$key = Rexenv_Sync_Pairing::create();
			} elseif ( 'disconnect' === $action ) {
				Rexenv_Sync_Pairing::delete();
			}
		}
		$paired = Rexenv_Sync_Pairing::get();
		$https  = 0 === strpos( home_url(), 'https://' );
		echo '<div class="wrap"><h1>rexenv Sync</h1>';
		if ( ! $https ) {
			echo '<div class="notice notice-error"><p>' . esc_html__( 'This site is not served over HTTPS. rexenv will not copy a database over a plain connection — turn on HTTPS first.', 'rexenv-sync' ) . '</p></div>';
		}
		if ( $key ) {
			echo '<p>' . esc_html__( 'Paste this key into rexenv (New site → From a live site). It is shown only now; generating a new one replaces it.', 'rexenv-sync' ) . '</p>';
			echo '<p><textarea readonly rows="3" class="large-text code" onclick="this.select()">' . esc_textarea( $key ) . '</textarea></p>';
		} elseif ( $paired ) {
			/* translators: %s: the key id */
			echo '<p>' . esc_html( sprintf( __( 'Connected (key %s).', 'rexenv-sync' ), $paired['key_id'] ) ) . '</p>';
		} else {
			echo '<p>' . esc_html__( 'Not connected. Nothing is shared until you connect.', 'rexenv-sync' ) . '</p>';
		}
		echo '<form method="post">';
		wp_nonce_field( 'rexsync' );
		echo '<button class="button button-primary" name="rexsync_action" value="connect">' . esc_html( $paired ? __( 'Regenerate key', 'rexenv-sync' ) : __( 'Connect to rexenv', 'rexenv-sync' ) ) . '</button> ';
		if ( $paired ) {
			echo '<button class="button" name="rexsync_action" value="disconnect">' . esc_html__( 'Disconnect', 'rexenv-sync' ) . '</button>';
		}
		echo '</form></div>';
	}
}

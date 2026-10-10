<?php
/**
 * Plugin Name:       rexenv Sync
 * Description:       Connects this site to rexenv on a developer's machine, so it can be copied there and kept in step. Every request is signed; nothing is sent until you connect.
 * Version:           0.1.0
 * Requires at least: 6.0
 * Requires PHP:      7.4
 * License:           GPL-2.0-or-later
 * License URI:       https://www.gnu.org/licenses/gpl-2.0.html
 * Text Domain:       rexenv-sync
 *
 * @package rexenv-sync
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

define( 'REXENV_SYNC_VERSION', '0.1.0' );
define( 'REXENV_SYNC_DIR', __DIR__ );

require_once __DIR__ . '/includes/class-rexenv-sync-signature.php';
require_once __DIR__ . '/includes/class-rexenv-sync-pairing.php';
require_once __DIR__ . '/includes/class-rexenv-sync-reader.php';
require_once __DIR__ . '/includes/class-rexenv-sync-pusher.php';
require_once __DIR__ . '/includes/class-rexenv-sync-rest.php';

add_action( 'rest_api_init', array( 'Rexenv_Sync_Rest', 'register' ) );
add_filter( 'rest_pre_serve_request', array( 'Rexenv_Sync_Rest', 'serve_raw' ), 10, 4 );

if ( is_admin() ) {
	require_once __DIR__ . '/includes/class-rexenv-sync-admin.php';
	add_action( 'admin_menu', array( 'Rexenv_Sync_Admin', 'menu' ) );
}

register_uninstall_hook( __FILE__, array( 'Rexenv_Sync_Pairing', 'delete' ) );

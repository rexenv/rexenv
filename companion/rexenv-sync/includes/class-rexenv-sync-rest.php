<?php
/**
 * The REST surface (docs/rexsync-protocol.md §4) — thin: every route's
 * permission_callback IS the §3 signature check, and the handlers hand their
 * arguments to Rexenv_Sync_Reader.
 *
 * @package rexenv-sync
 * @license GPL-2.0-or-later
 */

if ( ! defined( 'ABSPATH' ) ) {
	exit;
}

final class Rexenv_Sync_Rest {

	const NS = 'rexenv-sync/v1';

	/** The routes, so a test can assert that every one is signed. */
	public static function routes() {
		return array(
			'/manifest'   => array( 'GET', 'manifest' ),
			'/files/list' => array( 'GET', 'files_list' ),
			'/files/read' => array( 'POST', 'files_read' ),
			'/db/export'  => array( 'GET', 'db_export' ),
		);
	}

	public static function register() {
		foreach ( self::routes() as $route => $spec ) {
			register_rest_route(
				self::NS,
				$route,
				array(
					'methods'             => $spec[0],
					'callback'            => array( __CLASS__, $spec[1] ),
					'permission_callback' => array( __CLASS__, 'authorize' ),
				)
			);
		}
	}

	/**
	 * §3, in its order, before any argument is read. A refusal is a WP_Error with
	 * the protocol's code (§6) and, for clock_skew, the plugin's clock.
	 */
	public static function authorize( WP_REST_Request $request ) {
		$headers = array();
		foreach ( array( 'x-rexsync-key', 'x-rexsync-ts', 'x-rexsync-nonce', 'x-rexsync-sig' ) as $h ) {
			$headers[ $h ] = (string) $request->get_header( $h );
		}
		$query = array();
		foreach ( $request->get_query_params() as $k => $v ) {
			if ( 'rest_route' === $k || is_array( $v ) ) {
				continue;
			}
			$query[] = array( (string) $k, (string) $v );
		}
		$now  = time();
		$code = Rexenv_Sync_Signature::check(
			$headers,
			$request->get_method(),
			$request->get_route(),
			$query,
			(string) $request->get_body(),
			Rexenv_Sync_Pairing::get(),
			$now,
			array( 'Rexenv_Sync_Pairing', 'nonce_seen' )
		);
		if ( null === $code ) {
			return true;
		}
		$messages = array(
			'unknown_key'   => 'This site has no rexenv pairing with that key — paste the current key into rexenv.',
			'clock_skew'    => 'The request time is too far from this site\'s clock.',
			'bad_signature' => 'The request signature does not match this site\'s pairing.',
			'replayed'      => 'This request was already used.',
		);
		return new WP_Error( $code, $messages[ $code ], array( 'status' => 401, 'now' => $now ) );
	}

	/** `exclude` globs from rexenv (§5), comma-separated. */
	private static function globs( WP_REST_Request $request ) {
		$raw = (string) $request->get_param( 'exclude' );
		return '' === $raw ? array() : array_values( array_filter( array_map( 'trim', explode( ',', $raw ) ) ) );
	}

	public static function manifest() {
		return rest_ensure_response( Rexenv_Sync_Reader::manifest() );
	}

	public static function files_list( WP_REST_Request $request ) {
		return rest_ensure_response( Rexenv_Sync_Reader::list_files( $request->get_param( 'cursor' ), self::globs( $request ) ) );
	}

	public static function files_read( WP_REST_Request $request ) {
		$json  = json_decode( (string) $request->get_body(), true );
		$paths = is_array( $json ) && isset( $json['paths'] ) && is_array( $json['paths'] ) ? $json['paths'] : array();
		$resp  = new WP_REST_Response( array( 'raw' => Rexenv_Sync_Reader::file_frame( $paths, self::globs( $request ) ) ) );
		$resp->header( 'Content-Type', 'application/octet-stream' );
		$resp->header( 'X-Rexsync-Raw', '1' );
		return $resp;
	}

	public static function db_export( WP_REST_Request $request ) {
		return rest_ensure_response( Rexenv_Sync_Reader::export_table( (string) $request->get_param( 'table' ), $request->get_param( 'cursor' ) ) );
	}

	/** Send the file frame as raw bytes, not JSON (§4.3). */
	public static function serve_raw( $served, $result, $request, $server ) {
		if ( $served || ! ( $result instanceof WP_REST_Response ) ) {
			return $served;
		}
		$headers = $result->get_headers();
		if ( empty( $headers['X-Rexsync-Raw'] ) ) {
			return $served;
		}
		$data = $result->get_data();
		echo $data['raw']; // phpcs:ignore WordPress.Security.EscapeOutput -- a binary frame, not HTML
		return true;
	}
}

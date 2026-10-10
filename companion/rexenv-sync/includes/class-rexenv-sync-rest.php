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
			'/push/begin'    => array( 'POST', 'push_begin' ),
			'/push/file'     => array( 'POST', 'push_file' ),
			'/push/db'       => array( 'POST', 'push_db' ),
			'/push/swap'     => array( 'POST', 'push_swap' ),
			'/push/rollback' => array( 'POST', 'push_rollback' ),
			'/push/abort'    => array( 'POST', 'push_abort' ),
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
		// `now` ONLY on clock_skew — which only a caller who signed can reach (#837).
		// On every other refusal the body says nothing about this site's clock.
		$data = array( 'status' => 401 );
		if ( 'clock_skew' === $code ) {
			$data['now'] = $now;
		}
		return new WP_Error( $code, $messages[ $code ], $data );
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
		$since = $request->get_param( 'uploads_since' );
		return rest_ensure_response( Rexenv_Sync_Reader::list_files( $request->get_param( 'cursor' ), self::globs( $request ), ( null === $since || '' === $since ) ? null : (int) $since ) );
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

	private static function body_json( WP_REST_Request $request ) {
		$j = json_decode( (string) $request->get_body(), true );
		return is_array( $j ) ? $j : array();
	}

	private static function strings( $v ) {
		return is_array( $v ) ? array_values( array_filter( array_map( 'strval', $v ), 'strlen' ) ) : array();
	}

	public static function push_begin( WP_REST_Request $request ) {
		$j = self::body_json( $request );
		return rest_ensure_response( Rexenv_Sync_Pusher::begin(
			self::strings( isset( $j['tables'] ) ? $j['tables'] : null ),
			self::strings( isset( $j['files'] ) ? $j['files'] : null ),
			isset( $j['base'] ) && is_array( $j['base'] ) ? $j['base'] : array(),
			self::strings( isset( $j['override'] ) ? $j['override'] : null )
		) );
	}

	public static function push_file( WP_REST_Request $request ) {
		return rest_ensure_response( Rexenv_Sync_Pusher::receive_file( (string) $request->get_param( 'push_id' ), (string) $request->get_param( 'path' ), $request->get_param( 'offset' ), (string) $request->get_body() ) );
	}

	public static function push_db( WP_REST_Request $request ) {
		$j = self::body_json( $request );
		return rest_ensure_response( Rexenv_Sync_Pusher::receive_sql( (string) $request->get_param( 'push_id' ), (string) $request->get_param( 'table' ), isset( $j['sql'] ) ? (string) $j['sql'] : '', isset( $j['sha256'] ) ? (string) $j['sha256'] : '' ) );
	}

	public static function push_swap( WP_REST_Request $request ) {
		return rest_ensure_response( Rexenv_Sync_Pusher::swap( (string) $request->get_param( 'push_id' ) ) );
	}

	public static function push_rollback( WP_REST_Request $request ) {
		return rest_ensure_response( Rexenv_Sync_Pusher::rollback( (string) $request->get_param( 'backup_id' ) ) );
	}

	public static function push_abort( WP_REST_Request $request ) {
		return rest_ensure_response( Rexenv_Sync_Pusher::abort( (string) $request->get_param( 'push_id' ) ) );
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

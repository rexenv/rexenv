<?php
/**
 * rexsync1 request signatures — the plugin's half of docs/rexsync-protocol.md §3.
 *
 * Pure: the caller passes the clock and the nonce store, so tests/signature-test.php
 * can check it against the same vectors file the rexenv client's Rust tests read.
 *
 * @package rexenv-sync
 * @license GPL-2.0-or-later
 */

if ( ! defined( 'ABSPATH' ) && PHP_SAPI !== 'cli' ) {
	exit;
}

final class Rexenv_Sync_Signature {

	const PROTOCOL   = 'rexsync1';
	const MAX_SKEW   = 300;
	const NONCE_TTL  = 600;

	/** base64url without padding. */
	public static function b64u_encode( $bytes ) {
		return rtrim( strtr( base64_encode( $bytes ), '+/', '-_' ), '=' );
	}

	/** Decode base64url; false on anything that is not. */
	public static function b64u_decode( $text ) {
		if ( ! is_string( $text ) || preg_match( '/[^A-Za-z0-9_-]/', $text ) ) {
			return false;
		}
		return base64_decode( strtr( $text, '-_', '+/' ) . str_repeat( '=', ( 4 - strlen( $text ) % 4 ) % 4 ), true );
	}

	/** RFC 3986: everything but the unreserved set is percent-encoded. */
	private static function pct( $s ) {
		return rawurlencode( (string) $s ); // rawurlencode leaves exactly A-Z a-z 0-9 - . _ ~
	}

	/**
	 * The canonical string: seven lines, the query sorted by key then value.
	 *
	 * @param string $method HTTP method.
	 * @param string $path   The REST route as sent.
	 * @param array  $query  List of [key, value] pairs, in any order.
	 * @param string $ts     X-Rexsync-Ts.
	 * @param string $nonce  X-Rexsync-Nonce.
	 * @param string $body   The raw request body.
	 */
	public static function canonical( $method, $path, array $query, $ts, $nonce, $body ) {
		$pairs = array();
		foreach ( $query as $pair ) {
			$pairs[] = array( self::pct( $pair[0] ), self::pct( $pair[1] ) );
		}
		usort(
			$pairs,
			function ( $a, $b ) {
				$c = strcmp( $a[0], $b[0] );
				return 0 !== $c ? $c : strcmp( $a[1], $b[1] );
			}
		);
		$q = implode(
			'&',
			array_map(
				function ( $p ) {
					return $p[0] . '=' . $p[1];
				},
				$pairs
			)
		);
		return implode( "\n", array( self::PROTOCOL, strtoupper( $method ), $path, $q, $ts, $nonce, hash( 'sha256', $body ) ) );
	}

	public static function sign( $secret, $canonical ) {
		return self::b64u_encode( hash_hmac( 'sha256', $canonical, $secret, true ) );
	}

	/**
	 * The §3 check, in its order. Returns null when the request is good, or the
	 * error code to answer with. `$seen` is called with the nonce key and must
	 * return true when it was seen within NONCE_TTL — and record it otherwise.
	 *
	 * @param array    $headers  Lower-cased header names => values.
	 * @param array    $pairing  ['key_id' => ..., 'secret' => raw bytes] or empty.
	 * @param int      $now      Unix seconds.
	 * @param callable $seen     function( string $nonce_key ): bool.
	 */
	public static function check( array $headers, $method, $path, array $query, $body, array $pairing, $now, $seen ) {
		foreach ( array( 'x-rexsync-key', 'x-rexsync-ts', 'x-rexsync-nonce', 'x-rexsync-sig' ) as $h ) {
			if ( ! isset( $headers[ $h ] ) || '' === $headers[ $h ] ) {
				return 'unknown_key';
			}
		}
		if ( empty( $pairing['key_id'] ) || ! hash_equals( (string) $pairing['key_id'], (string) $headers['x-rexsync-key'] ) ) {
			return 'unknown_key';
		}
		// The signature BEFORE the clock (10 Oct 2026, the security review): the
		// skew refusal carries this site's exact time, and the key id's validity
		// with it. Both are for a caller who holds the secret, not for anyone who
		// guessed a key id. A stale ts still fails here when the caller cannot
		// sign; a signed-but-stale one is told the time below.
		$ts       = (string) $headers['x-rexsync-ts'];
		$expected = self::sign( $pairing['secret'], self::canonical( $method, $path, $query, $ts, $headers['x-rexsync-nonce'], $body ) );
		if ( ! hash_equals( $expected, (string) $headers['x-rexsync-sig'] ) ) {
			return 'bad_signature';
		}
		if ( ! ctype_digit( $ts ) || abs( (int) $now - (int) $ts ) > self::MAX_SKEW ) {
			return 'clock_skew';
		}
		if ( call_user_func( $seen, hash( 'sha256', $pairing['key_id'] . '|' . $headers['x-rexsync-nonce'] ) ) ) {
			return 'replayed';
		}
		return null;
	}
}

<?php
/**
 * Tools → rexenv Sync: the one page a site owner sees. Three cards — where this
 * site stands (connected or not, what rexenv last did here, the kept backup),
 * the three steps to connect, and the site's own facts (HTTPS, versions, free
 * disk). The key is shown ONCE, right after Connect / Regenerate, with a Copy
 * button; Regenerate and Disconnect ask first, because either one cuts the
 * developer's rexenv off until the new key is pasted.
 *
 * Everything here is read from this site; nothing phones anywhere (plan §11 Q1).
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

	/** "3 minutes ago" / "never" — WordPress's own words. */
	private static function ago( $at ) {
		if ( empty( $at ) ) {
			return __( 'never', 'rexenv-sync' );
		}
		/* translators: %s: a human time difference, e.g. "3 mins" */
		return sprintf( __( '%s ago', 'rexenv-sync' ), human_time_diff( (int) $at, time() ) );
	}

	/** Bytes under `$dir`, bounded to 20 000 entries so a page load stays a page load. */
	private static function dir_bytes( $dir ) {
		if ( ! is_dir( $dir ) ) {
			return 0;
		}
		$bytes = 0;
		$n     = 0;
		try {
			$it = new RecursiveIteratorIterator( new RecursiveDirectoryIterator( $dir, FilesystemIterator::SKIP_DOTS ) );
			foreach ( $it as $f ) {
				if ( $f->isFile() ) {
					$bytes += $f->getSize();
				}
				if ( ++$n > 20000 ) {
					break;
				}
			}
		} catch ( Exception $e ) { // phpcs:ignore Generic.CodeAnalysis.EmptyStatement
			// unreadable: the size is a courtesy, not a fact the page rests on
		}
		return $bytes;
	}

	public static function page() {
		if ( ! current_user_can( 'manage_options' ) ) {
			return;
		}
		$key    = null;
		$notice = null;
		if ( isset( $_POST['rexsync_action'] ) && check_admin_referer( 'rexsync' ) ) {
			$action = sanitize_key( wp_unslash( $_POST['rexsync_action'] ) );
			if ( 'connect' === $action ) {
				$key = Rexenv_Sync_Pairing::create();
			} elseif ( 'disconnect' === $action ) {
				Rexenv_Sync_Pairing::delete();
				$notice = __( 'Disconnected. rexenv can no longer read or write this site until you connect again.', 'rexenv-sync' );
			}
		}
		$paired = Rexenv_Sync_Pairing::get();
		$row    = get_option( Rexenv_Sync_Pairing::OPTION );
		$last   = Rexenv_Sync_Pairing::last();
		$keep   = get_option( Rexenv_Sync_Pusher::KEEP );
		$https  = 0 === strpos( home_url(), 'https://' );
		$free   = function_exists( 'disk_free_space' ) ? @disk_free_space( WP_CONTENT_DIR ) : false; // phpcs:ignore WordPress.PHP.NoSilencedErrors
		global $wpdb;

		echo '<div class="wrap rexsync-wrap"><h1>rexenv Sync</h1>';
		echo '<style>
			.rexsync-wrap .rexsync-cards{display:grid;grid-template-columns:repeat(auto-fit,minmax(20rem,1fr));gap:16px;max-width:72rem;margin-top:12px}
			.rexsync-card{background:#fff;border:1px solid #c3c4c7;border-radius:4px;padding:16px 20px;box-shadow:0 1px 1px rgba(0,0,0,.04)}
			.rexsync-card h2{margin:0 0 8px;font-size:14px;text-transform:uppercase;letter-spacing:.04em;color:#50575e}
			.rexsync-state{display:flex;align-items:center;gap:10px;font-size:16px;font-weight:600;margin:4px 0 12px}
			.rexsync-dot{width:12px;height:12px;border-radius:50%;background:#dcdcde;flex:none}
			.rexsync-dot.on{background:#00a32a}.rexsync-dot.warn{background:#dba617}
			.rexsync-card dl{display:grid;grid-template-columns:max-content 1fr;gap:6px 14px;margin:0}
			.rexsync-card dt{color:#646970}.rexsync-card dd{margin:0}
			.rexsync-card code{font-size:12px}
			.rexsync-steps{margin:0;padding-left:1.25em}.rexsync-steps li{margin-bottom:6px}
			.rexsync-key textarea{font-family:Menlo,Consolas,monospace;font-size:12px;word-break:break-all}
			.rexsync-actions{margin-top:14px;display:flex;gap:8px;flex-wrap:wrap}
			.rexsync-muted{color:#646970}
		</style>';
		if ( ! $https ) {
			echo '<div class="notice notice-error"><p>' . esc_html__( 'This site is not served over HTTPS. rexenv will not copy a database over a plain connection — turn on HTTPS first.', 'rexenv-sync' ) . '</p></div>';
		}
		if ( $notice ) {
			echo '<div class="notice notice-info is-dismissible"><p>' . esc_html( $notice ) . '</p></div>';
		}

		// The key, once.
		if ( $key ) {
			echo '<div class="notice notice-success rexsync-key" style="padding:12px 16px"><p><strong>' . esc_html__( 'Your connection key — shown only now.', 'rexenv-sync' ) . '</strong> '
				. esc_html__( 'In rexenv open the site\'s Live tab (or New site → From a live site), paste it and press Connect. Generating a new one replaces it.', 'rexenv-sync' ) . '</p>';
			echo '<p><textarea readonly rows="3" class="large-text code" id="rexsync-key" onclick="this.select()">' . esc_textarea( $key ) . '</textarea></p>';
			echo '<p><button type="button" class="button" onclick="var t=document.getElementById(\'rexsync-key\');t.select();(navigator.clipboard?navigator.clipboard.writeText(t.value):Promise.reject()).then(function(){document.getElementById(\'rexsync-copied\').textContent=\'' . esc_js( __( 'Copied.', 'rexenv-sync' ) ) . '\'},function(){document.execCommand(\'copy\');document.getElementById(\'rexsync-copied\').textContent=\'' . esc_js( __( 'Copied.', 'rexenv-sync' ) ) . '\'})">'
				. esc_html__( 'Copy key', 'rexenv-sync' ) . '</button> <span id="rexsync-copied" class="rexsync-muted"></span></p></div>';
		}

		echo '<div class="rexsync-cards">';

		// ── Card 1: where this site stands ─────────────────────────────────
		echo '<div class="rexsync-card"><h2>' . esc_html__( 'Connection', 'rexenv-sync' ) . '</h2>';
		if ( $paired ) {
			$seen_at = isset( $last['seen']['at'] ) ? (int) $last['seen']['at'] : 0;
			$dot     = $seen_at ? 'on' : 'warn';
			echo '<div class="rexsync-state"><span class="rexsync-dot ' . esc_attr( $dot ) . '"></span>' . esc_html__( 'Connected to rexenv', 'rexenv-sync' ) . '</div>';
			echo '<dl>';
			echo '<dt>' . esc_html__( 'Key', 'rexenv-sync' ) . '</dt><dd><code>' . esc_html( $paired['key_id'] ) . '</code>'
				. ( isset( $row['created'] ) ? ' <span class="rexsync-muted">' . esc_html( sprintf( /* translators: %s: "3 days ago" */ __( 'made %s', 'rexenv-sync' ), self::ago( $row['created'] ) ) ) . '</span>' : '' ) . '</dd>';
			echo '<dt>' . esc_html__( 'Last contact', 'rexenv-sync' ) . '</dt><dd>' . esc_html( self::ago( $seen_at ) )
				. ( $seen_at ? '' : ' <span class="rexsync-muted">' . esc_html__( '— the key has not been pasted into rexenv yet', 'rexenv-sync' ) . '</span>' ) . '</dd>';
			echo '<dt>' . esc_html__( 'Last pull', 'rexenv-sync' ) . '</dt><dd>' . esc_html( self::ago( isset( $last['pull']['at'] ) ? $last['pull']['at'] : 0 ) )
				. ' <span class="rexsync-muted">' . esc_html__( '(rexenv copied this site to the developer\'s machine)', 'rexenv-sync' ) . '</span></dd>';
			echo '<dt>' . esc_html__( 'Last push', 'rexenv-sync' ) . '</dt><dd>' . esc_html( self::ago( isset( $last['push']['at'] ) ? $last['push']['at'] : 0 ) );
			if ( isset( $last['push']['at'] ) ) {
				echo ' <span class="rexsync-muted">' . esc_html( sprintf( /* translators: 1: tables, 2: files */ __( '(%1$d tables, %2$d files replaced here)', 'rexenv-sync' ), (int) $last['push']['tables'], (int) $last['push']['files'] ) ) . '</span>';
			}
			echo '</dd>';
			if ( isset( $last['rollback']['at'] ) ) {
				echo '<dt>' . esc_html__( 'Last roll back', 'rexenv-sync' ) . '</dt><dd>' . esc_html( self::ago( $last['rollback']['at'] ) ) . '</dd>';
			}
			echo '<dt>' . esc_html__( 'Backup kept', 'rexenv-sync' ) . '</dt><dd>';
			if ( is_array( $keep ) && ! empty( $keep['id'] ) ) {
				$bytes = self::dir_bytes( WP_CONTENT_DIR . '/rexsync-backups/' . $keep['id'] );
				echo '<code>' . esc_html( substr( $keep['id'], 0, 8 ) ) . '…</code> '
					. esc_html( sprintf( /* translators: 1: "2 hours ago", 2: tables, 3: files, 4: size */ __( 'from the push %1$s — %2$d tables, %3$d files (%4$s on disk). rexenv can roll this site back to it.', 'rexenv-sync' ), self::ago( $keep['at'] ), count( (array) $keep['tables'] ), count( (array) $keep['files'] ), size_format( $bytes ) ) );
			} else {
				echo '<span class="rexsync-muted">' . esc_html__( 'none — a push makes one; the next push replaces it', 'rexenv-sync' ) . '</span>';
			}
			echo '</dd></dl>';
		} else {
			echo '<div class="rexsync-state"><span class="rexsync-dot"></span>' . esc_html__( 'Not connected', 'rexenv-sync' ) . '</div>';
			echo '<p>' . esc_html__( 'Nothing is shared until you connect. Connecting makes a key; only a rexenv holding that key can read this site, and every request it sends is signed.', 'rexenv-sync' ) . '</p>';
		}
		echo '<form method="post" class="rexsync-actions">';
		wp_nonce_field( 'rexsync' );
		if ( $paired ) {
			echo '<button class="button" name="rexsync_action" value="connect" onclick="return confirm(' . esc_attr( wp_json_encode( __( 'Make a new key? The current one stops working at once — the developer must paste the new key into rexenv before the next pull or push.', 'rexenv-sync' ) ) ) . ')">' . esc_html__( 'Regenerate key', 'rexenv-sync' ) . '</button>';
			echo '<button class="button button-link-delete" name="rexsync_action" value="disconnect" onclick="return confirm(' . esc_attr( wp_json_encode( __( 'Disconnect? rexenv can no longer read or write this site. The kept backup stays on disk.', 'rexenv-sync' ) ) ) . ')">' . esc_html__( 'Disconnect', 'rexenv-sync' ) . '</button>';
		} else {
			echo '<button class="button button-primary" name="rexsync_action" value="connect"' . ( $https ? '' : ' disabled title="' . esc_attr__( 'Turn on HTTPS first', 'rexenv-sync' ) . '"' ) . '>' . esc_html__( 'Connect to rexenv', 'rexenv-sync' ) . '</button>';
		}
		echo '</form></div>';

		// ── Card 2: how to use it ──────────────────────────────────────────
		echo '<div class="rexsync-card"><h2>' . esc_html__( 'How it works', 'rexenv-sync' ) . '</h2><ol class="rexsync-steps">';
		echo '<li>' . esc_html__( 'Press Connect to rexenv. A key appears once — copy it.', 'rexenv-sync' ) . '</li>';
		echo '<li>' . esc_html__( 'In rexenv on your machine: New site → From a live site (or an existing site\'s Live tab) → paste the key → Connect.', 'rexenv-sync' ) . '</li>';
		echo '<li>' . esc_html__( 'Pull brings this site to your machine; Push sends your changes here, keeping a backup you can roll back to. The Live tab\'s Status says whether either side has changes.', 'rexenv-sync' ) . '</li>';
		echo '</ol><p class="rexsync-muted">' . esc_html__( 'This site never connects out. rexenv calls in over HTTPS with a signed request, and only while it holds the key.', 'rexenv-sync' ) . '</p></div>';

		// ── Card 3: the site's own facts ───────────────────────────────────
		echo '<div class="rexsync-card"><h2>' . esc_html__( 'This site', 'rexenv-sync' ) . '</h2><dl>';
		echo '<dt>' . esc_html__( 'Address', 'rexenv-sync' ) . '</dt><dd><code>' . esc_html( untrailingslashit( home_url() ) ) . '</code>'
			. ( $https ? '' : ' <strong style="color:#d63638">' . esc_html__( 'not HTTPS', 'rexenv-sync' ) . '</strong>' ) . '</dd>';
		echo '<dt>' . esc_html__( 'Versions', 'rexenv-sync' ) . '</dt><dd>' . esc_html( sprintf( 'WordPress %s · PHP %s · MySQL %s · plugin %s', get_bloginfo( 'version' ), PHP_VERSION, $wpdb->db_version(), REXENV_SYNC_VERSION ) ) . '</dd>';
		echo '<dt>' . esc_html__( 'Tables', 'rexenv-sync' ) . '</dt><dd>' . esc_html( sprintf( /* translators: 1: count, 2: prefix */ __( '%1$d with the prefix %2$s', 'rexenv-sync' ), count( Rexenv_Sync_Reader::manifest()['tables'] ), $wpdb->base_prefix ) ) . ( is_multisite() ? ' <span class="rexsync-muted">' . esc_html__( '(multisite — rexenv cannot copy a network yet)', 'rexenv-sync' ) . '</span>' : '' ) . '</dd>';
		echo '<dt>' . esc_html__( 'Free disk', 'rexenv-sync' ) . '</dt><dd>' . esc_html( false === $free ? __( 'unknown', 'rexenv-sync' ) : size_format( (int) $free ) ) . ' <span class="rexsync-muted">' . esc_html__( '(a push needs about twice the pushed database free)', 'rexenv-sync' ) . '</span></dd>';
		echo '<dt>' . esc_html__( 'REST', 'rexenv-sync' ) . '</dt><dd><code>' . esc_html( home_url( '/?rest_route=/rexenv-sync/v1/manifest' ) ) . '</code> <span class="rexsync-muted">' . esc_html__( '(answers 401 to anyone without the key — that is expected)', 'rexenv-sync' ) . '</span></dd>';
		echo '</dl></div>';

		echo '</div></div>';
	}
}

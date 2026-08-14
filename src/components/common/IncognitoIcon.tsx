/** The private-browsing glyph — hat + spy glasses, the mark every browser's
 *  private window has trained people to read as "this visit isn't recorded".
 *
 *  Hand-drawn because lucide has no incognito icon: its nearest neighbour is a
 *  masquerade mask, and a carnival mask is not what someone is scanning that
 *  row for. Sized to lucide's 24px viewBox and drawn in `currentColor`, so it
 *  inherits hover and disabled colour exactly like the icons beside it.
 *
 *  Filled hat, OUTLINED lenses — decided by rendering it, not by taste. Chrome's
 *  own mark fills the lenses too; at the 15px this actually ships at, two filled
 *  discs joined by a bar read as a dumbbell, while rings read as glasses at
 *  every size. The hat stays solid because an outlined crown at 15px turns into
 *  a tunnel.
 *
 *  NOT a brand mark, and it must not become one: no vendor hex, and no chasing
 *  Chrome's proportions pixel for pixel. The same glyph rides the Firefox and
 *  Brave rows, whose vendors draw private mode entirely differently — what is
 *  shared is the IDEA, which is what an interface glyph is for. */
export function IncognitoIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" className={className}>
      {/* Crown, then a brim wider than it — a hat reads as a hat only when the
          brim overhangs. */}
      <path d="M12 3.6c-2.4 0-4.2 1.7-4.2 3.9V10h8.4V7.5c0-2.2-1.8-3.9-4.2-3.9z" />
      <rect x="3" y="10.3" width="18" height="1.8" rx=".9" />
      {/* Lenses + bridge, kept inside the brim's width: glasses wider than the
          hat read as a face wearing a hat, not as a disguise. */}
      <g fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round">
        <circle cx="7.9" cy="16.6" r="2.5" />
        <circle cx="16.1" cy="16.6" r="2.5" />
        <path d="M10.4 16.1c.9-.6 2.3-.6 3.2 0" />
      </g>
    </svg>
  );
}

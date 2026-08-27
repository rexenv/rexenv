<?php
/**
 * The page rexenv generates for a new Blank PHP site. It is a starting point,
 * not a framework: one file, no dependencies, and the sample query below is the
 * whole of the database code.
 *
 * Everything here is yours — edit it, or delete it and start from an empty file.
 * rexenv writes this page once, at create time, and never touches it again.
 */
declare(strict_types=1);

/** Escape for HTML. Every value below goes through it — including the ones that
 *  look safe, because a site name and a table row are both user input. */
function rex_e(?string $value): string
{
    return htmlspecialchars((string) $value, ENT_QUOTES, 'UTF-8');
}

// The connection rexenv generated, when it generated one. `db.php` returns a
// ready PDO and defines REXENV_DB with the credentials it used.
$db      = null;
$rows    = [];
$dbError = null;
if (is_file(__DIR__ . '/db.php')) {
    try {
        $pdo   = require __DIR__ . '/db.php';
        $db    = REXENV_DB;
        // The table name comes from a file on disk, not a request — but it is
        // interpolated into SQL, so it is filtered anyway. Identifiers cannot be
        // bound as parameters; this is the substitute.
        $table = preg_replace('/[^A-Za-z0-9_]/', '', (string) $db['table']);
        $rows  = $pdo->query("SELECT `id`, `title`, `note`, `created_at` FROM `{$table}` ORDER BY `id`")
            ->fetchAll();
    } catch (Throwable $e) {
        $dbError = $e->getMessage();
    }
}

$host    = (string) ($_SERVER['HTTP_HOST'] ?? 'localhost');
$scheme  = (string) ($_SERVER['HTTP_X_FORWARDED_PROTO'] ?? (empty($_SERVER['HTTPS']) ? 'http' : 'https'));
$server  = (string) ($_SERVER['SERVER_SOFTWARE'] ?? 'unknown');
$docroot = (string) ($_SERVER['DOCUMENT_ROOT'] ?? __DIR__);
?>
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title><?= rex_e($host) ?> — rexenv</title>
<style>
  :root {
    color-scheme: dark;
    --bg: #0d0e12;
    --surface: #15171d;
    --surface-2: #1c1f27;
    --well: #0b0c10;
    --border: #262a33;
    --border-subtle: #20242c;
    --text: #e7e9ee;
    --muted: #8a90a0;
    --dim: #6e7681;
    --brand: #7c5cff;
    --brand-deep: #6b4ae8;
    --brand-tint: #c9bcff;
    --ok: #3fb950;
    --ok-bg: rgba(63, 185, 80, 0.12);
    --ok-border: rgba(63, 185, 80, 0.22);
    --err: #f85149;
    --err-bg: rgba(248, 81, 73, 0.12);
    --err-border: rgba(248, 81, 73, 0.24);
    --mono: ui-monospace, "SF Mono", SFMono-Regular, "JetBrains Mono", Menlo, monospace;
    --sans: -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, system-ui, sans-serif;
  }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    background: var(--bg);
    color: var(--text);
    font: 15px/1.6 var(--sans);
    -webkit-font-smoothing: antialiased;
  }
  /* One soft violet wash behind the fold — the app's hero gradient, at rest. */
  body::before {
    content: "";
    position: fixed;
    inset: -30% -10% auto -10%;
    height: 70vh;
    background: radial-gradient(60% 60% at 50% 0%, rgba(124, 92, 255, 0.16), transparent 70%);
    pointer-events: none;
  }
  main { position: relative; max-width: 860px; margin: 0 auto; padding: 68px 24px 96px; }
  header { display: flex; align-items: center; gap: 11px; }
  .mark {
    display: grid; place-items: center;
    width: 38px; height: 38px;
    border: 1px solid #2c303b; border-radius: 10px;
    background: linear-gradient(160deg, #20232c, #13151b);
  }
  .mark svg { width: 21px; height: 21px; display: block; }
  .wordmark { font-weight: 600; letter-spacing: -0.01em; }
  .wordmark span { color: var(--muted); font-weight: 400; }
  .chip {
    margin-left: auto;
    font: 11px/1 var(--mono);
    letter-spacing: 0.08em; text-transform: uppercase;
    color: var(--brand-tint);
    background: rgba(124, 92, 255, 0.14);
    border: 1px solid rgba(124, 92, 255, 0.28);
    border-radius: 999px; padding: 7px 11px;
  }
  h1 {
    margin: 34px 0 0;
    font-size: 34px; line-height: 1.2; font-weight: 600; letter-spacing: -0.02em;
  }
  h1 .domain { font-family: var(--mono); font-size: 30px; color: var(--brand-tint); }
  .lede { margin: 12px 0 0; max-width: 62ch; color: var(--muted); }
  .lede code {
    font-family: var(--mono); font-size: 13px; color: var(--text);
    background: var(--well); border: 1px solid var(--border-subtle);
    border-radius: 5px; padding: 2px 6px;
  }
  .facts {
    margin-top: 30px;
    display: grid; gap: 10px;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  }
  .fact {
    background: var(--surface); border: 1px solid var(--border-subtle);
    border-radius: 10px; padding: 13px 15px; min-width: 0;
  }
  .fact dt {
    font: 10px/1 var(--mono); letter-spacing: 0.13em; text-transform: uppercase;
    color: var(--dim);
  }
  .fact dd {
    margin: 9px 0 0; font-family: var(--mono); font-size: 13px;
    overflow-wrap: anywhere;
  }
  section.card {
    margin-top: 26px;
    background: var(--surface); border: 1px solid var(--border);
    border-radius: 12px; overflow: hidden;
  }
  .card-head {
    display: flex; align-items: center; gap: 10px; flex-wrap: wrap;
    padding: 15px 18px; border-bottom: 1px solid var(--border-subtle);
    background: var(--surface-2);
  }
  .card-head h2 { margin: 0; font-size: 14px; font-weight: 600; }
  .pill {
    font: 11px/1 var(--mono); border-radius: 999px; padding: 5px 9px;
    color: var(--muted); background: var(--well); border: 1px solid var(--border-subtle);
  }
  .pill.ok { color: var(--ok); background: var(--ok-bg); border-color: var(--ok-border); }
  .pill.err { color: var(--err); background: var(--err-bg); border-color: var(--err-border); }
  .pill.spacer { margin-left: auto; }
  table { width: 100%; border-collapse: collapse; }
  th, td { text-align: left; padding: 12px 18px; border-bottom: 1px solid var(--border-subtle); }
  th {
    font: 10px/1 var(--mono); letter-spacing: 0.13em; text-transform: uppercase;
    color: var(--dim); font-weight: 400;
  }
  tbody tr:last-child td { border-bottom: 0; }
  td.id { font-family: var(--mono); color: var(--dim); width: 1%; }
  td .title { font-weight: 600; }
  td .note { color: var(--muted); font-size: 13.5px; }
  td.when { font-family: var(--mono); font-size: 12px; color: var(--dim); white-space: nowrap; }
  .card-note {
    padding: 14px 18px; color: var(--muted); font-size: 13px;
    background: var(--well); border-top: 1px solid var(--border-subtle);
  }
  .card-note code { font-family: var(--mono); font-size: 12.5px; color: var(--brand-tint); }
  .body { padding: 18px; }
  .body p { margin: 0 0 12px; color: var(--muted); }
  .body p:last-child { margin-bottom: 0; }
  pre {
    margin: 0; padding: 14px 16px; overflow-x: auto;
    background: var(--well); border: 1px solid var(--border-subtle); border-radius: 9px;
    font: 12.5px/1.7 var(--mono); color: var(--text);
  }
  .err-text { color: var(--err); font-family: var(--mono); font-size: 12.5px; overflow-wrap: anywhere; }
  footer {
    margin-top: 40px; padding-top: 18px; border-top: 1px solid var(--border-subtle);
    display: flex; gap: 12px; flex-wrap: wrap; align-items: baseline;
    color: var(--dim); font-size: 12.5px;
  }
  footer .path { font-family: var(--mono); overflow-wrap: anywhere; }
  footer .by { margin-left: auto; }
  footer .by b { color: var(--muted); font-weight: 600; }
  @media (max-width: 620px) {
    main { padding: 44px 18px 64px; }
    h1, h1 .domain { font-size: 25px; }
    th:last-child, td.when { display: none; }
  }
</style>
</head>
<body>
<main>
  <header>
    <div class="mark" aria-hidden="true">
      <svg viewBox="0 0 1222.6667 1222.6667" xmlns="http://www.w3.org/2000/svg">
        <path fill="#734ffb" d="m 614.66671,46.66667 -74.66667,110.66667 v 1.33334 c 8.68038,11.93929 27.82674,33.08887 29.10286,48 4.29846,50.22665 -58.67184,91.7037 -103.76954,84.10596 -21.60608,-3.64005 -55.90283,-19.48421 -57.52673,-45.43929 -0.66366,-10.60759 8.71602,-21.111 12.1934,-30.66667 l -130.66668,-44 23.07717,65.33334 59.5895,161.33334 c 28.25054,-8.13509 54.30335,-24.48511 82.66668,-33.29932 78.38106,-24.35775 162.96306,-32.49251 244.00001,-17.88175 31.13803,5.61409 62.5009,13.2032 92.00001,24.78499 19.9585,7.83602 39.57601,20.51253 60,26.39608 l 81.33334,-226.66668 -128.00001,44 c 2.03801,11.43294 14.02588,22.6648 10.92904,34.66667 -2.79671,10.83854 -11.65625,20.42953 -20.26237,27.09359 -34.97795,27.08496 -85.1779,14.85107 -113.10186,-16.42692 -11.03544,-12.36092 -27.6746,-41.61117 -19.28499,-58.66667 5.02026,-10.2059 12.30534,-19.87598 18.60905,-29.33334 2.71513,-4.07349 7.76542,-9.40731 7.20479,-14.66667 -1.0297,-9.65958 -13.41878,-22.65438 -18.76033,-30.66666 C 652.64112,101.62826 636.81327,66.962407 614.66671,46.66667 M 294.66669,440.00003 c 15.72607,25.48324 33.61011,49.74854 50.22221,74.66667 5.34989,8.02482 12.47835,24.90389 21.8313,28.52881 7.40271,2.86906 18.74984,0.80453 26.61316,0.80453 h 58.66667 200.00001 c 45.59221,0 88.78875,-4.26587 123.77264,30.89302 48.59945,48.84254 32.6101,131.30222 -23.77263,166.35292 -28.44572,17.68343 -59.88623,16.0874 -92.00001,16.0874 h -100 c -32.21814,0 -64.53988,-0.17057 -94.66668,13.14917 -53.6993,23.74171 -70.53027,78.23341 -88.00411,129.51751 -28.44869,83.49431 -58.91631,167.03844 -90.66256,249.33334 h 105.33334 c 8.83329,0 30.5317,3.9725 37.73458,-1.3704 4.09847,-3.0401 5.81669,-11.3772 7.5319,-15.9629 4.82064,-12.8883 9.51131,-25.8355 14.48144,-38.6667 19.24468,-49.6833 38.07602,-99.53442 57.01958,-149.33334 6.31873,-16.61068 11.72978,-48.36198 27.25309,-58.48869 9.55372,-6.23234 22.45464,-4.17798 33.31275,-4.17798 h 73.33334 c 79.37769,0 156.24529,2.70663 222.66668,-48.3138 107.46013,-82.54363 116.80901,-251.8187 31.41464,-353.01956 -40.44963,-47.93677 -99.55958,-75.84278 -159.41465,-90.80656 -84.62195,-21.15552 -173.32854,-18.20468 -257.33335,3.98559 -25.60714,6.76424 -50.73694,16.55892 -74.66667,27.86215 -11.39832,5.38403 -22.63416,14.48437 -34.66667,18.09358 -21.78463,6.53443 -53.23623,0.86524 -76,0.86524 m 281.33335,468.00003 54.44031,61.33334 96.00102,106.6667 48.22535,53.3333 20.22632,20.463 51.77368,9.9413 140.00001,24.2624 c -10.8859,-23.4822 -38.50488,-44.5594 -55.6193,-64 -42.64299,-48.4392 -85.73365,-96.6684 -129.59572,-144.00004 -14.22396,-15.34912 -28.49861,-30.87011 -42.21916,-46.66666 -5.46557,-6.29265 -12.56933,-17.54314 -20.57096,-20.52881 -13.28988,-4.95899 -35.15804,-0.80453 -49.32821,-0.80453 z"/>
      </svg>
    </div>
    <div class="wordmark">rexenv <span>· local development</span></div>
    <div class="chip">Blank PHP</div>
  </header>

  <h1><span class="domain"><?= rex_e($host) ?></span> is serving.</h1>
  <p class="lede">
    This page is <code>index.php</code> in your site folder. PHP, the web server and
    the TLS certificate are already running — edit the file, refresh, and keep going.
  </p>

  <dl class="facts">
    <div class="fact"><dt>PHP</dt><dd><?= rex_e(PHP_VERSION) ?></dd></div>
    <div class="fact"><dt>Web server</dt><dd><?= rex_e($server) ?></dd></div>
    <div class="fact"><dt>Scheme</dt><dd><?= rex_e($scheme) ?></dd></div>
    <div class="fact"><dt>Database</dt><dd><?= $db ? rex_e($db['engine']) : 'none' ?></dd></div>
  </dl>

<?php if ($db && $dbError === null): ?>
  <section class="card">
    <div class="card-head">
      <h2>Sample data</h2>
      <span class="pill"><?= rex_e($db['database']) ?></span>
      <span class="pill">table <?= rex_e((string) $db['table']) ?></span>
      <span class="pill ok spacer">connected</span>
    </div>
    <table>
      <thead><tr><th>#</th><th>Row</th><th>Seeded</th></tr></thead>
      <tbody>
<?php foreach ($rows as $row): ?>
        <tr>
          <td class="id"><?= rex_e((string) $row['id']) ?></td>
          <td>
            <div class="title"><?= rex_e((string) $row['title']) ?></div>
            <div class="note"><?= rex_e((string) $row['note']) ?></div>
          </td>
          <td class="when"><?= rex_e((string) $row['created_at']) ?></td>
        </tr>
<?php endforeach; ?>
<?php if (!$rows): ?>
        <tr><td class="id">—</td><td><div class="note">The table is empty. Insert a row and refresh.</div></td><td class="when"></td></tr>
<?php endif; ?>
      </tbody>
    </table>
    <div class="card-note">
      rexenv created the database and seeded this table when the site was created.
      The connection lives in <code>db.php</code>; the query is one line at the top of
      <code>index.php</code>. Open the site's <b>Database</b> tab in rexenv to browse it in Adminer.
    </div>
  </section>
<?php elseif ($db): ?>
  <section class="card">
    <div class="card-head">
      <h2>Sample data</h2>
      <span class="pill"><?= rex_e($db['database']) ?></span>
      <span class="pill err spacer">not connected</span>
    </div>
    <div class="body">
      <p>
        <code>db.php</code> is here, but the query did not run. The usual reason is that
        the database service is stopped — start <?= rex_e($db['engine']) ?> from the
        <b>Services</b> screen in rexenv, then refresh this page.
      </p>
      <p class="err-text"><?= rex_e($dbError) ?></p>
    </div>
  </section>
<?php else: ?>
  <section class="card">
    <div class="card-head">
      <h2>No database</h2>
      <span class="pill spacer">db.php not found</span>
    </div>
    <div class="body">
      <p>
        This site was created without one. rexenv can create a database, seed a sample
        table and generate the connection for you — pick MySQL or MariaDB in the
        <b>Database</b> field when you create a Blank PHP site.
      </p>
      <p>To wire one up by hand, a <code>db.php</code> next to this file is all it takes:</p>
      <pre>&lt;?php
return new PDO(
    'mysql:host=127.0.0.1;port=13306;dbname=my_database;charset=utf8mb4',
    'root',
    ''
);</pre>
    </div>
  </section>
<?php endif; ?>

  <footer>
    <span class="path"><?= rex_e($docroot) ?></span>
    <span class="by">served by <b>rexenv</b></span>
  </footer>
</main>
</body>
</html>

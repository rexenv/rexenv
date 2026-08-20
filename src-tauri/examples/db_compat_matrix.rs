//! Print the whole compatibility matrix (Stage 2 step 4). Run:
//! `cargo run --example db_compat_matrix`
//!
//! Pure computation — no server, no filesystem, nothing to clean up. It exists
//! so the table and its wording can be reviewed as the user will read it,
//! rather than inferred from the code.

use rexenv_lib::core::dbcompat::{compat, Source, Target, Verdict, Version};
use rexenv_lib::core::dbsource::Vendor;

fn source(vendor: Option<Vendor>, v: Option<&str>) -> Source {
    Source { vendor, version: v.and_then(Version::parse) }
}

fn main() {
    let targets = [
        (Vendor::Mysql, "8.4.6"),
        (Vendor::Mysql, "8.0.44"),
        (Vendor::Mariadb, "12.3.2"),
        (Vendor::Mariadb, "11.4.12"),
    ];
    let sources = [
        ("MySQL 5.6.51", source(Some(Vendor::Mysql), Some("5.6.51-log"))),
        ("MySQL 5.7.44", source(Some(Vendor::Mysql), Some("5.7.44"))),
        ("MySQL 8.0.27 (DBngin)", source(Some(Vendor::Mysql), Some("8.0.27"))),
        ("Percona 8.0.36-28", source(Some(Vendor::Mysql), Some("8.0.36-28"))),
        ("MySQL 8.4.6", source(Some(Vendor::Mysql), Some("8.4.6"))),
        ("MySQL 9.1.0", source(Some(Vendor::Mysql), Some("9.1.0"))),
        ("MariaDB 10.6.21", source(Some(Vendor::Mariadb), Some("10.6.21-MariaDB"))),
        ("MariaDB 11.4.12", source(Some(Vendor::Mariadb), Some("11.4.12-MariaDB"))),
        ("MariaDB 12.3.2", source(Some(Vendor::Mariadb), Some("12.3.2-MariaDB"))),
        ("MariaDB 12.9 (future)", source(Some(Vendor::Mariadb), Some("12.9.0-MariaDB"))),
        ("unidentified", source(None, None)),
        ("MySQL, no version", source(Some(Vendor::Mysql), None)),
    ];

    println!("{:<24}{:<16}{:<16}{:<16}MariaDB 11.4.12", "source \\ target", "MySQL 8.4.6", "MySQL 8.0.44", "MariaDB 12.3.2");
    println!("{}", "-".repeat(96));
    for (name, s) in &sources {
        print!("{name:<24}");
        for (vendor, v) in &targets {
            let t = Target { vendor: *vendor, version: Version::parse(v).unwrap() };
            let verdict = compat(s, &t);
            let cell = match &verdict {
                Verdict::Proceed { cautions } if cautions.is_empty() => "ok".to_string(),
                Verdict::Proceed { cautions } => format!("ok (+{})", cautions.len()),
                Verdict::NeedsOverride { .. } => "override".to_string(),
                Verdict::Blocked { .. } => "blocked".to_string(),
            };
            print!("{cell:<16}");
        }
        println!();
    }

    println!("\n\nHow each non-clean verdict reads, against MySQL 8.4.6:\n");
    for (name, s) in &sources {
        let t = Target { vendor: Vendor::Mysql, version: Version::parse("8.4.6").unwrap() };
        let v = compat(s, &t);
        if matches!(&v, Verdict::Proceed { cautions } if cautions.is_empty()) {
            continue;
        }
        println!("── {name} → MySQL 8.4.6  [{}]", v.label());
        for line in textwrap(&v.explain(), 92) {
            println!("   {line}");
        }
        println!();
    }

    println!("── and the two cross-vendor directions, side by side:\n");
    for (name, s, t) in [
        (
            "MariaDB 11.4.12 → MySQL 8.4.6",
            source(Some(Vendor::Mariadb), Some("11.4.12-MariaDB")),
            Target { vendor: Vendor::Mysql, version: Version::parse("8.4.6").unwrap() },
        ),
        (
            "MySQL 8.0.27 → MariaDB 12.3.2",
            source(Some(Vendor::Mysql), Some("8.0.27")),
            Target { vendor: Vendor::Mariadb, version: Version::parse("12.3.2").unwrap() },
        ),
    ] {
        let v = compat(&s, &t);
        println!("── {name}  [{}]  runs_now={} overridable={}", v.label(), v.runs_now(), v.overridable());
        for line in textwrap(&v.explain(), 92) {
            println!("   {line}");
        }
        println!();
    }
}

fn textwrap(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in s.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

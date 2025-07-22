use clap::{Parser, ValueEnum};
use fake::{Fake, Faker};
use indicatif::{ProgressBar, ProgressStyle};
use rand::seq::SliceRandom;
use rand::Rng;
use std::io::{Result as IoResult, Write};
use url;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Size of the fake database (small, medium, large)
    #[arg(long, value_enum, default_value_t = DbSize::Small)]
    size: DbSize,

    /// Output SQL file (default: fake_{size}.sql)
    #[arg(long)]
    out: Option<String>,

    /// Automatically load the SQL file into the database after generation
    #[arg(long)]
    load: bool,

    /// Database URL (e.g. postgres://user:pass@localhost:5432/dbname)
    #[arg(long)]
    db_url: Option<String>,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum DbSize {
    Small,
    Medium,
    Large,
}

impl DbSize {
    fn row_counts(&self) -> (usize, usize) {
        match self {
            DbSize::Small => (5000, 30000), // users, orders
            DbSize::Medium => (50000, 200000),
            DbSize::Large => (2000000, 10000000),
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (user_count, order_count) = args.size.row_counts();
    let out_file = args
        .out
        .unwrap_or_else(|| format!("fake_{:?}.sql", args.size).to_lowercase());
    let need_generate = !std::path::Path::new(&out_file).exists();
    if need_generate {
        let mut out = std::fs::File::create(&out_file)?;
        write_schema(&mut out)?;
        write_users(&mut out, user_count)?;
        write_orders(&mut out, order_count, user_count)?;
        println!("SQL file generated: {}", out_file);
    } else {
        println!("SQL file already exists: {}", out_file);
    }
    println!("You can load it with: psql < {}", out_file);
    if args.load {
        let db_url = args
            .db_url
            .as_ref()
            .expect("--db-url must be provided with --load");
        load_sql_file(&out_file, db_url)?;
    }
    Ok(())
}

fn load_sql_file(sql_path: &str, db_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Check for psql in PATH
    if which::which("psql").is_err() {
        return Err(
            "psql command not found in PATH. Please install PostgreSQL client tools.".into(),
        );
    }

    // Parse the db_url for psql args
    let url = url::Url::parse(db_url)?;
    let user = url.username();
    if user.is_empty() {
        return Err("Database URL must contain a username".into());
    }
    let password = url.password();
    let host = url.host_str().unwrap_or("localhost");
    let port = url.port().unwrap_or(5432);
    let dbname = url.path().trim_start_matches('/');
    if dbname.is_empty() {
        return Err("Database URL must contain a database name".into());
    }

    let mut cmd = std::process::Command::new("psql");
    cmd.arg("-h")
        .arg(host)
        .arg("-p")
        .arg(port.to_string())
        .arg("-U")
        .arg(user)
        .arg("-d")
        .arg(dbname)
        .arg("-f")
        .arg(sql_path);
    if let Some(pw) = password {
        cmd.env("PGPASSWORD", pw);
    }
    println!("\nLoading SQL file into database with psql...");
    let status = cmd.status()?;
    if status.success() {
        println!("Database loaded successfully.");
    } else {
        eprintln!("psql failed with exit code: {}", status);
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn write_schema<W: Write>(out: &mut W) -> IoResult<()> {
    writeln!(out, "DROP TABLE IF EXISTS orders;")?;
    writeln!(out, "DROP TABLE IF EXISTS users;")?;
    writeln!(out, "CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE);")?;
    writeln!(out, "CREATE TABLE orders (id SERIAL PRIMARY KEY, user_id INTEGER REFERENCES users(id), product TEXT NOT NULL, amount INTEGER NOT NULL);")?;
    Ok(())
}

fn write_users<W: Write>(out: &mut W, user_count: usize) -> IoResult<()> {
    let pb = ProgressBar::new(user_count as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} users written")
        .unwrap()
        .progress_chars("#>-"));
    writeln!(
        out,
        "COPY users (id, name, email) FROM stdin WITH (FORMAT text);"
    )?;
    for i in 0..user_count {
        let name: String = Faker.fake();
        let email = format!("user{}@example.com", i + 1);
        let name = name.replace('\t', " ").replace('\n', " ");
        let email = email.replace('\t', " ").replace('\n', " ");
        writeln!(out, "{}\t{}\t{}", i + 1, name, email)?;
        pb.inc(1);
    }
    writeln!(out, "\\.")?;
    pb.finish_with_message("Users written");
    Ok(())
}

fn write_orders<W: Write>(out: &mut W, order_count: usize, user_count: usize) -> IoResult<()> {
    let pb = ProgressBar::new(order_count as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} orders written")
        .unwrap()
        .progress_chars("#>-"));
    let products = ["Widget", "Gadget", "Thingamajig", "Doodad", "Gizmo"];
    let mut rng = rand::thread_rng();
    writeln!(
        out,
        "COPY orders (id, user_id, product, amount) FROM stdin WITH (FORMAT text);"
    )?;
    for i in 0..order_count {
        let user_id = rng.gen_range(1..=user_count);
        let product = products.choose(&mut rng).unwrap_or(&"Widget");
        let amount = rng.gen_range(1..=10);
        // Tab-separated, escaping tabs and newlines
        let product = product.replace('\t', " ").replace('\n', " ");
        writeln!(out, "{}\t{}\t{}\t{}", i + 1, user_id, product, amount)?;
        pb.inc(1);
    }
    writeln!(out, "\\.")?;
    pb.finish_with_message("Orders written");
    Ok(())
}

use clap::{Parser, ValueEnum};
use fake::{Fake, Faker};
use rand::Rng;
use rand::seq::SliceRandom;
use std::io::{Write, Result as IoResult};
use indicatif::{ProgressBar, ProgressStyle};
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
    /// Returns the number of users and orders associated with the database size.
    ///
    /// The tuple contains the user count as the first element and the order count as the second.
    ///
    /// # Examples
    ///
    /// ```
    /// let (users, orders) = DbSize::Medium.row_counts();
    /// assert_eq!(users, 50000);
    /// assert_eq!(orders, 200000);
    /// ```
    fn row_counts(&self) -> (usize, usize) {
        match self {
            DbSize::Small => (5000, 30000),     // users, orders
            DbSize::Medium => (50000, 200000),
            DbSize::Large => (2000000, 10000000),
        }
    }
}

/// Entry point for the fake SQL database generator CLI tool.
///
/// Parses command-line arguments to determine the database size, output file, and loading options. Generates a SQL dump file with fake users and orders if it does not already exist, and optionally loads it into a PostgreSQL database if requested.
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if file operations or database loading fail.
///
/// # Examples
///
/// ```sh
/// cargo run -- --size medium --out mydb.sql --load --db-url postgres://user:pass@localhost/db
/// ```
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let (user_count, order_count) = args.size.row_counts();
    let out_file = args.out.unwrap_or_else(|| format!("fake_{:?}.sql", args.size).to_lowercase());
    let need_generate = !std::path::Path::new(&out_file).exists();
    if need_generate {
        let mut out = std::fs::File::create(&out_file)?;
        write_schema(&mut out)?;
        let users = generate_users(user_count);
        write_users(&mut out, &users)?;
        write_orders(&mut out, order_count, users.len())?;
        println!("SQL file generated: {}", out_file);
    } else {
        println!("SQL file already exists: {}", out_file);
    }
    println!("You can load it with: psql < {}", out_file);
    if args.load {
        let db_url = args.db_url.as_ref().expect("--db-url must be provided with --load");
        load_sql_file(&out_file, db_url)?;
    }
    Ok(())
}

/// Loads a SQL file into a PostgreSQL database using the `psql` command-line tool.
///
/// Parses the provided database URL to extract connection parameters and executes `psql` to import the specified SQL file. The password, if present in the URL, is set via the `PGPASSWORD` environment variable.
///
/// # Arguments
///
/// * `sql_path` - Path to the SQL file to be loaded.
/// * `db_url` - PostgreSQL connection URL.
///
/// # Errors
///
/// Returns an error if the database URL cannot be parsed or if the `psql` command fails to execute.
fn load_sql_file(sql_path: &str, db_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Parse the db_url for psql args
    let url = url::Url::parse(db_url)?;
    let user = url.username();
    let password = url.password();
    let host = url.host_str().unwrap_or("localhost");
    let port = url.port().unwrap_or(5432);
    let dbname = url.path().trim_start_matches('/');
    let mut cmd = std::process::Command::new("psql");
    cmd.arg("-h").arg(host)
        .arg("-p").arg(port.to_string())
        .arg("-U").arg(user)
        .arg("-d").arg(dbname)
        .arg("-f").arg(sql_path);
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

/// Writes SQL statements to define the schema for `users` and `orders` tables.
///
/// Drops existing tables if they exist, then creates new `users` and `orders` tables with appropriate columns and constraints.
///
/// # Examples
///
/// ```
/// use std::io::Cursor;
/// let mut buffer = Cursor::new(Vec::new());
/// write_schema(&mut buffer).unwrap();
/// let sql = String::from_utf8(buffer.into_inner()).unwrap();
/// assert!(sql.contains("CREATE TABLE users"));
/// ```
fn write_schema<W: Write>(out: &mut W) -> IoResult<()> {
    writeln!(out, "DROP TABLE IF EXISTS orders;")?;
    writeln!(out, "DROP TABLE IF EXISTS users;")?;
    writeln!(out, "CREATE TABLE users (id SERIAL PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL UNIQUE);")?;
    writeln!(out, "CREATE TABLE orders (id SERIAL PRIMARY KEY, user_id INTEGER REFERENCES users(id), product TEXT NOT NULL, amount INTEGER NOT NULL);")?;
    Ok(())
}

/// Generates a vector of fake user names and emails.
///
/// Each user has a randomly generated name and a unique email in the format `user{n}@example.com`.
///
/// # Parameters
/// - `count`: The number of users to generate.
///
/// # Returns
/// A vector of `(name, email)` tuples representing the generated users.
///
/// # Examples
///
/// ```
/// let users = generate_users(3);
/// assert_eq!(users.len(), 3);
/// assert!(users[0].1.starts_with("user1@"));
/// ```
fn generate_users(count: usize) -> Vec<(String, String)> {
    let pb = ProgressBar::new(count as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} users")
        .unwrap()
        .progress_chars("#>-")
    );
    let mut users = Vec::with_capacity(count);
    for i in 0..count {
        let name: String = Faker.fake();
        let email = format!("user{}@example.com", i + 1);
        users.push((name, email));
        pb.inc(1);
    }
    pb.finish_with_message("Users generated");
    users
}

/// Writes SQL `INSERT` statements for a list of users to the provided output.
///
/// Each user is assigned a sequential ID starting from 1. Single quotes in names and emails are escaped for SQL safety.
///
/// # Parameters
/// - `out`: The writable output stream to which SQL statements are written.
/// - `users`: A slice of `(name, email)` tuples representing users.
///
/// # Returns
/// Returns an I/O result indicating success or failure.
///
/// # Examples
///
/// ```
/// use std::io::Cursor;
/// let users = vec![("Alice O'Neil".to_string(), "alice@example.com".to_string())];
/// let mut buf = Cursor::new(Vec::new());
/// write_users(&mut buf, &users).unwrap();
/// let sql = String::from_utf8(buf.into_inner()).unwrap();
/// assert!(sql.contains("INSERT INTO users"));
/// ```
fn write_users<W: Write>(out: &mut W, users: &[(String, String)]) -> IoResult<()> {
    let pb = ProgressBar::new(users.len() as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} users written")
        .unwrap()
        .progress_chars("#>-"));
    for (i, (name, email)) in users.iter().enumerate() {
        let name = name.replace("'", "''");
        let email = email.replace("'", "''");
        writeln!(out, "INSERT INTO users (id, name, email) VALUES ({}, '{}', '{}');", i + 1, name, email)?;
        pb.inc(1);
    }
    pb.finish_with_message("Users written");
    Ok(())
}

/// Writes SQL insert statements for a specified number of orders with randomized data.
///
/// Each order is assigned a random user ID, product name, and amount, and is written as an SQL `INSERT` statement to the provided output stream.
///
/// # Parameters
/// - `order_count`: The number of orders to generate and write.
/// - `user_count`: The total number of users to select random user IDs from.
///
/// # Examples
///
/// ```
/// use std::fs::File;
/// let mut file = tempfile::tempfile().unwrap();
/// write_orders(&mut file, 10, 5).unwrap();
/// ```
fn write_orders<W: Write>(out: &mut W, order_count: usize, user_count: usize) -> IoResult<()> {
    let pb = ProgressBar::new(order_count as u64);
    pb.set_style(ProgressStyle::default_bar()
        .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} orders written")
        .unwrap()
        .progress_chars("#>-"));
    let products = ["Widget", "Gadget", "Thingamajig", "Doodad", "Gizmo"];
    let mut rng = rand::thread_rng();
    for i in 0..order_count {
        let user_id = rng.gen_range(1..=user_count);
        let product = products.choose(&mut rng).unwrap_or(&"Widget");
        let amount = rng.gen_range(1..=10);
        writeln!(out, "INSERT INTO orders (id, user_id, product, amount) VALUES ({}, {}, '{}', {});", i + 1, user_id, product, amount)?;
        pb.inc(1);
    }
    pb.finish_with_message("Orders written");
    Ok(())
}

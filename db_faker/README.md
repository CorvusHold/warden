# db_faker

A utility to generate and load fake PostgreSQL databases for CI and non-regression testing.

## Features
- Generates SQL files with a configurable amount of fake data for small, medium, or large test databases.
- Deterministic, fast generation (unique emails, random names).
- Progress bars for large data sets.
- Can automatically load the generated SQL file into a PostgreSQL database using `psql`.
- Perfect for CI pipelines and reproducible local testing.

## Usage

### Build
```sh
cargo build --release -p db_faker
```

### Generate SQL only
```sh
./target/release/db_faker --size medium
```
- Generates `fake_medium.sql` in the current directory (if not already present).
- Sizes: `small` (5k users), `medium` (50k users), `large` (2M users) with proportionate orders.

### Generate and load into a database
```sh
./target/release/db_faker --size medium --load --db-url "postgres://user:pass@localhost:5432/dbname"
```
- Generates (or reuses) the SQL file, then loads it into the specified database using `psql`.

### Options
- `--size [small|medium|large]` : Choose the database size. Default is `small`.
- `--out <file.sql>` : Specify a custom output SQL file name.
- `--load` : If set, automatically loads the SQL file into the database.
- `--db-url <url>` : Database URL (required with `--load`).

## Integration
- Can be called from CI scripts or GitHub Actions before running tests.
- Useful for benchmarking and regression testing with different DB sizes.

## Schema
- `users(id, name, email)`
- `orders(id, user_id, product, amount)`

## Dependencies
- [fake](https://crates.io/crates/fake)
- [clap](https://crates.io/crates/clap)
- [rand](https://crates.io/crates/rand)
- [indicatif](https://crates.io/crates/indicatif)
- [url](https://crates.io/crates/url)

## Example: In CI
```yaml
- name: Install latest PostgreSQL client
  run: |
    sudo apt-get update
    sudo apt-get install -y postgresql-client
- name: Generate and load fake database
  run: cargo run --release -p db_faker -- --size medium --load --db-url "$DATABASE_URL"
  env:
    DATABASE_URL: "postgres://user:pass@localhost:5432/dbname"
```

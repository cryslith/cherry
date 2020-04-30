use barrel::{types, Migration, SqlVariant};
use log::{debug, info};
use quaint::ast::{Insert, ParameterizedValue, Select};
use quaint::connector::{Queryable, TransactionCapable};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MigrationError {
  #[error("database error")]
  DB(#[from] quaint::error::Error),
  #[error("expected 0 or 1 rows in _migration, got `{0}`")]
  TooMuchState(usize),
  #[error("bad migration state: invalid state `{0}`")]
  BadRow(String),
  #[error("bad migration state: expected number in range [0, `{0}`], got `{1}`")]
  OutOfRange(usize, i64),
  #[error("bad migration name: expected `{0}`, got `{1}`")]
  IncorrectMigrationName(String, String),
}

async fn sql(
  db: &(impl Queryable + TransactionCapable),
  sql: &[&str],
) -> Result<(), MigrationError> {
  let transaction = db.start_transaction().await?;
  for cmd in sql {
    debug!("sql: {}", cmd);
    transaction.raw_cmd(cmd).await?;
  }
  transaction.commit().await?;
  Ok(())
}

pub async fn migrate(
  db: &(impl Queryable + TransactionCapable),
  variant: SqlVariant,
  migrations: &[(String, Migration)],
) -> Result<(), MigrationError> {
  let mut m = Migration::new();
  // single-row table
  m.create_table_if_not_exists("_migration", |t| {
    // Number of migrations applied
    t.add_column("number", types::integer());
    // Name of latest migration applied (for error-checking)
    t.add_column("name", types::varchar(255));
  });
  info!("initializing migration table");
  sql(db, &[m.make_from(variant).as_str()]).await?;

  let current_state = db
    .select(
      Select::from_table("_migration")
        .column("number")
        .column("name"),
    )
    .await?;
  if current_state.len() > 1 {
    return Err(MigrationError::TooMuchState(current_state.len()));
  }
  let (current_number, current_name) = match current_state.first() {
    Some(row) => match (
      row.get("number").and_then(ParameterizedValue::as_i64),
      row.get("name").and_then(ParameterizedValue::to_string),
    ) {
      (Some(number), Some(name)) => (number, name),
      _ => {
        return Err(MigrationError::BadRow(format!("{:?}", row)));
      }
    },

    None => {
      info!("no existing migration state; inserting initial state");
      db.insert(
        Insert::single_into("_migration")
          .value("number", 0usize)
          .value("name", "_initial")
          .build(),
      )
      .await?;
      (0, "_initial".to_string())
    }
  };

  let expected_name = if current_number == 0 {
    "_initial"
  } else {
    let (name, _) = migrations
      .get(current_number as usize - 1)
      .ok_or(MigrationError::OutOfRange(migrations.len(), current_number))?;
    name
  };
  if expected_name != current_name {
    return Err(MigrationError::IncorrectMigrationName(
      expected_name.to_string(),
      current_name,
    ));
  }

  for (name, _migration) in migrations {
    info!("running migration: {}", name);
    unimplemented!();
  }

  Ok(())
}

pub fn migrations() -> Vec<(String, Migration)> {
  vec![]
}

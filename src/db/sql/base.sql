-- name: selectVersion
SELECT current_setting('server_version') AS version;

-- name: schemaExists
SELECT 1 FROM pg_namespace WHERE nspname = '{SCHEMA}';

-- name: schemaCreate
CREATE SCHEMA {SCHEMA};

-- name: schemaInfoExists
SELECT 1 FROM pg_tables WHERE schemaname = '{SCHEMA}' and tablename = 'schema_info';

-- name: schemaInfoCreate
CREATE TABLE {SCHEMA}.schema_info (
  id int2 PRIMARY KEY DEFAULT 1 CHECK (id = 1),
  coin text NOT NULL,
  chain text NOT NULL,
  version int2 NOT NULL CHECK (version > 0),
  extra jsonb NOT NULL,
  stage text NOT NULL
);

-- name: schemaInfoInsert
INSERT INTO {SCHEMA}.schema_info (coin, chain, version, extra, stage) VALUES ($1, $2, $3, $4, $5);

-- name: schemaInfoSelect
SELECT coin, chain, version, extra, stage FROM {SCHEMA}.schema_info LIMIT 1;

-- name: schemaInfoSetStage
UPDATE {SCHEMA}.schema_info SET stage = $1;

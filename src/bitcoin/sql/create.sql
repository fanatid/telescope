-- log10(21*1e6*1e8) = ~15.32
-- name: btcValue
CREATE DOMAIN {SCHEMA}.btc_value AS numeric(30, 0);

-- name: blocks
CREATE TABLE {SCHEMA}.blocks (
  processed boolean NOT NULL DEFAULT FALSE,
  height int4 NOT NULL,
  hash bytea PRIMARY KEY,
  prev_hash bytea NOT NULL,
  next_hash bytea,
  size int4 NOT NULL,
  time timestamp without time zone NOT NULL,
  transactions_count int4 NOT NULL,
  inputs_count int4 NOT NULL,
  inputs_total {SCHEMA}.btc_value, -- `NOT NULL` will be added on transform
  outputs_count int4 NOT NULL,
  outputs_total {SCHEMA}.btc_value NOT NULL
);

-- We need initial, because otherwise `inputs_total` will be filled on transform.
-- Write to new table is faster than update and call `VACUUM`.
-- We do not do this for `blocks` because table size in that case is relatively small.
-- name: transactionsInitial
CREATE TABLE {SCHEMA}.transactions_initial (
  block_height int4 NOT NULL,
  index int4 NOT NULL,
  txid bytea NOT NULL,
  raw bytea NOT NULL,
  time timestamp without time zone NOT NULL,
  inputs_count int4 NOT NULL,
  -- inputs_total {SCHEMA}.btc_value NOT NULL,
  outputs_count int4 NOT NULL,
  outputs_total {SCHEMA}.btc_value NOT NULL
);

-- name: transactionsInitialBlockHeightIdx
CREATE INDEX transactions_initial_block_height_idx ON {SCHEMA}.transactions_initial (block_height);

-- name: transactions
CREATE TABLE {SCHEMA}.transactions (
  block_height int4,
  index int4,
  txid bytea NOT NULL,
  raw bytea NOT NULL,
  time timestamp without time zone NOT NULL,
  inputs_count int4 NOT NULL,
  inputs_total {SCHEMA}.btc_value NOT NULL,
  outputs_count int4 NOT NULL,
  outputs_total {SCHEMA}.btc_value NOT NULL
);

-- name: transactionsInputs
CREATE TABLE {SCHEMA}.transactions_inputs (
  block_height int4 NOT NULL,
  txid bytea NOT NULL,
  vin int4 NOT NULL,
  data text NOT NULL,
  output_txid bytea,
  output_vout int4
);

-- name: transactionsOutputs
CREATE TABLE {SCHEMA}.transactions_outputs (
  block_height int4 NOT NULL,
  txid bytea NOT NULL,
  vout int4 NOT NULL,
  data text NOT NULL
);

-- name: transactionsInputsOutputs
CREATE TABLE {SCHEMA}.transactions_inputs_outputs (
  input_block_height int4,
  input_txid bytea,
  input_vin int4,
  input_data text,
  output_block_height int4,
  output_txid bytea,
  output_vout int4,
  output_data text
);

-- name: addressHistory
CREATE TABLE {SCHEMA}.address_history (
  address text NOT NULL,
  block_height int4,
  txid bytea NOT NULL,
  tx_index int4, -- Used for pagination, when confirmed
  time timestamp without time zone NOT NULL, -- Used for pagination, when unconfirmed
  received {SCHEMA}.btc_value NOT NULL,
  sent {SCHEMA}.btc_value NOT NULL
);

-- name: addressUnspent
CREATE TABLE {SCHEMA}.address_unspent (
  address text NOT NULL,
  block_height int4,
  txid bytea NOT NULL,
  vout int4 NOT NULL,
  value {SCHEMA}.btc_value NOT NULL
);

-- name: statsAddresses
CREATE TABLE {SCHEMA}.stats_addresses (
  address text NOT NULL,
  count_history_confirmed int4 NOT NULL,
  count_history_unconfirmed int4 NOT NULL,
  count_unspent_confirmed int4 NOT NULL,
  count_unspent_unconfirmed int4 NOT NULL,
  received_confirmed {SCHEMA}.btc_value NOT NULL,
  received_unconfirmed {SCHEMA}.btc_value NOT NULL,
  sent_confirmed {SCHEMA}.btc_value NOT NULL,
  sent_unconfirmed {SCHEMA}.btc_value NOT NULL
);

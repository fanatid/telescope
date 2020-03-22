-- Blocks
-- name: blocksInsertOne
INSERT INTO {SCHEMA}.blocks (
  height, hash, prev_hash, next_hash,
  size, transactions_count,
  inputs_count, inputs_total, outputs_count, outputs_total
) VALUES (
  $1, $2, $3, $4,
  $5, $6,
  $7, $8, $9, $10
);

-- name: blocksSelectBestInfo
SELECT
  height, hash
FROM
  {SCHEMA}.blocks
ORDER BY
  height DESC
LIMIT
  1;

-- name: blocksDeleteByHeight
DELETE FROM {SCHEMA}.blocks WHERE height = $1;


-- Transactions, initial sync
-- name: transactionsInitialInsertMany
INSERT INTO {SCHEMA}.transactions_initial (
  block_height, index, txid, raw,
  time,
  inputs_count, outputs_count, outputs_total
) VALUES {VALUES};

-- name: transactionsInitialInsertInputs
INSERT INTO {SCHEMA}.transactions_inputs (
  block_height, txid, vin, data, output_txid, output_vout
) VALUES {VALUES};

-- name: transactionsInitialInsertOutputs
INSERT INTO {SCHEMA}.transactions_outputs (
  block_height, txid, vout, data
) VALUES {VALUES};


-- Transactions

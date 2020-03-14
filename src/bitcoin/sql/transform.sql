-- `getblock` method return next block hash, but only if we requested an Object.
-- I planning request raw block and decode it in Rust, instead decoding JSON object for performance,
-- so next blockhash will be unknown in this case. This function fill it for us.
-- name: blocksNextHashFill
DO
$BODY$
DECLARE
  nulls_cnt int4;
BEGIN
  WITH src AS (
    SELECT
      height,
      hash
    FROM {SCHEMA}.blocks
  ), latest AS (
    SELECT
      height
    FROM {SCHEMA}.blocks
    ORDER BY height DESC
    LIMIT 1
  )
  UPDATE {SCHEMA}.blocks
  SET
    next_hash = src.hash
  FROM src, latest
  WHERE
    {SCHEMA}.blocks.height < latest.height AND
    {SCHEMA}.blocks.height = src.height - 1;

  SELECT
    count(*)
  FROM {SCHEMA}.blocks
  WHERE
    next_hash IS NULL
  INTO STRICT nulls_cnt;

  IF nulls_cnt > 1 THEN
    RAISE EXCEPTION '{SCHEMA}.blocks still have nulled next_hash: %', nulls_cnt - 1;
  END IF;
END;
$BODY$ LANGUAGE plpgsql;


-- Merge inputs and outputs from different tables to one, so they will be linked.
-- name: transactionInputsOutputsFill
INSERT INTO {SCHEMA}.transactions_inputs_outputs (
  input_block_height,
  input_txid,
  input_vin,
  input_data,
  output_block_height,
  output_txid,
  output_vout,
  output_data
)
SELECT
  {SCHEMA}.transactions_inputs.block_height,
  {SCHEMA}.transactions_inputs.txid,
  {SCHEMA}.transactions_inputs.vin,
  {SCHEMA}.transactions_inputs.data,
  {SCHEMA}.transactions_outputs.block_height,
  {SCHEMA}.transactions_outputs.txid,
  {SCHEMA}.transactions_outputs.vout,
  {SCHEMA}.transactions_outputs.data
FROM
  {SCHEMA}.transactions_outputs
  FULL OUTER JOIN
    {SCHEMA}.transactions_inputs ON
      {SCHEMA}.transactions_outputs.txid = {SCHEMA}.transactions_inputs.output_txid AND
      {SCHEMA}.transactions_outputs.vout = {SCHEMA}.transactions_inputs.output_vout;

-- name: transactionsInputsDrop
DROP TABLE {SCHEMA}.transactions_inputs;

-- name: transactionsOutputsDrop
DROP TABLE {SCHEMA}.transactions_outputs;

-- name: transactionsInputsOutputsInputTxIdInputVinIdx
CREATE UNIQUE INDEX txs_inout_input_txid_input_vin_idx ON {SCHEMA}.transactions_inputs_outputs (input_txid, input_vin);

-- name: transactionsInputsOutputsOutputTxIdOutputVoutIdx
CREATE UNIQUE INDEX txs_inout_output_txid_output_vout_idx ON {SCHEMA}.transactions_inputs_outputs (output_txid, output_vout);


-- Call process function in loop from Rust in parallel.
-- name: blocksProcessed
SELECT
  processed,
  height
FROM
  {SCHEMA}.blocks;

-- name: blocksTransformCreate
CREATE OR REPLACE FUNCTION {SCHEMA}.blocks_transform (blk_height int4)
  RETURNS int4 AS
$BODY$
DECLARE
  query_count int4 = 1;
  blk_processed boolean;
  inputs_total {SCHEMA}.btc_value = 0;
  outputs_total {SCHEMA}.btc_value = 0;
  tx_row RECORD;
  tx_inputs_total {SCHEMA}.btc_value;
  addresses jsonb;
  row RECORD;
  row_value {SCHEMA}.btc_value;
  row_address text;
BEGIN
  -- Check that block is not transformed yet.
  SELECT processed FROM {SCHEMA}.blocks WHERE height = blk_height FOR UPDATE INTO STRICT blk_processed;
  IF blk_processed THEN
    RETURN query_count;
  END IF;

  -- Handle each transaction in block
  query_count := query_count + 1;
  FOR tx_row IN
    SELECT
      *
    FROM
      {SCHEMA}.transactions_draft
    WHERE
      block_height = blk_height
  LOOP
    tx_inputs_total = 0;
    addresses := '{}'::jsonb;

    -- Handle each input in transaction, except coinbase
    query_count := query_count + 1;
    FOR row IN
      SELECT
        input_data::jsonb,
        output_block_height,
        output_txid,
        output_vout,
        output_data::jsonb
      FROM
        {SCHEMA}.transactions_inputs_outputs
      WHERE
        input_txid = tx_row.txid AND output_txid IS NOT NULL
      ORDER BY
        input_vin
    LOOP
      row_value := (row.output_data->>'value')::{SCHEMA}.btc_value;
      tx_inputs_total := tx_inputs_total + row_value;

      FOR row_address IN SELECT jsonb_array_elements_text(row.output_data->'addresses') LOOP
        IF addresses ? row_address THEN
          addresses := jsonb_set(
            addresses,
            ARRAY[row_address],
            jsonb_set(
              addresses->row_address,
              '{sent}',
              to_jsonb((((addresses->row_address)->>'sent')::{SCHEMA}.btc_value + row_value)::text)
            )
          );
        ELSE
          addresses := jsonb_set(
            addresses,
            ARRAY[row_address],
            jsonb_build_object(
              'received', '0',
              'sent', to_jsonb(row.output_data->>'value')
            ),
            true
          );
        END IF;
      END LOOP;
    END LOOP;

    -- Handle each output in transaction
    query_count := query_count + 1;
    FOR row IN
      SELECT
        input_block_height,
        input_txid,
        input_vin,
        output_vout,
        output_data::jsonb
      FROM
        {SCHEMA}.transactions_inputs_outputs
      WHERE
        output_txid = tx_row.txid
      ORDER BY
        output_vout
    LOOP
      row_value := (row.output_data->>'value')::{SCHEMA}.btc_value;

      FOR row_address IN SELECT jsonb_array_elements_text(row.output_data->'addresses') LOOP
        IF addresses ? row_address THEN
          addresses := jsonb_set(
            addresses,
            ARRAY[row_address],
            jsonb_set(
              addresses->row_address,
              '{received}',
              to_jsonb((((addresses->row_address)->>'received')::{SCHEMA}.btc_value + row_value)::text)
            )
          );
        ELSE
          addresses := jsonb_set(
            addresses,
            ARRAY[row_address],
            jsonb_build_object(
              'received', to_jsonb(row.output_data->>'value'),
              'sent', '0'
            ),
            true
          );
        END IF;

        -- TODO: Is it possible collect to some variable and insert in one query?
        -- If output is not spent yet, insert it to unspent table.
        IF row.input_txid IS NULL THEN
          query_count := query_count + 1;
          INSERT INTO {SCHEMA}.address_unspent (
            address, block_height, txid, vout, value
          ) VALUES (
            row_address, blk_height, tx_row.txid, row.output_vout, row_value
          );
        END IF;
      END LOOP;
    END LOOP;

    -- TODO: Is it possible generate some collection for insert as one query?
    -- Insert address history
    FOR row_address IN SELECT jsonb_object_keys(addresses) LOOP
      query_count := query_count + 1;
      INSERT INTO {SCHEMA}.address_history (
        address, block_height, txid, tx_index, time, received, sent
      ) VALUES (
        row_address,
        blk_height,
        tx_row.txid,
        tx_row.index,
        tx_row.time,
        ((addresses->row_address)->>'received')::{SCHEMA}.btc_value,
        ((addresses->row_address)->>'sent')::{SCHEMA}.btc_value
      );
    END LOOP;

    -- Insert transaction with calculated `tx_inputs_total`
    query_count := query_count + 1;
    INSERT INTO {SCHEMA}.transactions (
      block_height, index, txid, raw, time,
      inputs_count, inputs_total, outputs_count, outputs_total
    ) VALUES (
      tx_row.block_height,
      tx_row.index,
      tx_row.txid,
      tx_row.raw,
      tx_row.time,
      tx_row.inputs_count,
      tx_inputs_total,
      tx_row.outputs_count,
      tx_row.outputs_total
    );

    inputs_total := inputs_total + tx_inputs_total;
    outputs_total := outputs_total + tx_row.outputs_total;
  END LOOP;

  -- Mark block as processed. Set total value for inputs and outputs.
  query_count := query_count + 1;
  UPDATE {SCHEMA}.blocks
  SET
    processed = TRUE,
    inputs_total = inputs_total,
    outputs_total = outputs_total
  WHERE
    height = blk_height;

  -- Return count of required queries.
  RETURN query_count;
END;
$BODY$ LANGUAGE plpgsql;

-- name: blocksTransformRun
SELECT {SCHEMA}.blocks_transform($1) AS count;

-- name: blocksTransformDrop
DROP FUNCTION {SCHEMA}.blocks_transform (int4);

-- name: blocksProcessedDrop
ALTER TABLE {SCHEMA}.blocks DROP COLUMN processed;

-- name: transactionsDraftDrop
DROP TABLE {SCHEMA}.transactions_draft;


-- Fill stats from address history & unspent
-- name: statsAddressesFill
INSERT INTO {SCHEMA}.stats_addresses (
  address,
  count_history_confirmed,
  count_history_unconfirmed,
  count_unspent_confirmed,
  count_unspent_unconfirmed,
  received_confirmed,
  received_unconfirmed,
  sent_confirmed,
  sent_unconfirmed
)
WITH history AS (
  SELECT
    address,
    count(*) AS count,
    sum(received) AS received,
    sum(sent) AS sent
  FROM
    {SCHEMA}.address_history
  GROUP BY
    address
), unspent AS (
  SELECT
    address,
    count(*) AS count
  FROM
    {SCHEMA}.address_unspent
  GROUP BY
    address
)
SELECT
  history.address,
  history.count, 0,
  COALESCE(unspent.count, 0), 0,
  history.received, '0',
  history.sent, '0'
FROM
  history
LEFT OUTER JOIN
  unspent ON unspent.address = history.address;


-- Add required pkeys, fkeys and indices
-- name: transactionsPKey
ALTER TABLE {SCHEMA}.transactions ADD PRIMARY KEY (txid);

-- name: transactionsBlockHeightIndexIdx
CREATE UNIQUE INDEX transactions_block_height_index_idx ON {SCHEMA}.transactions (block_height, index);

-- name: transactionsBlockHeightFKey
ALTER TABLE {SCHEMA}.transactions
ADD CONSTRAINT transactions_block_height_fkey
FOREIGN KEY (block_height)
REFERENCES {SCHEMA}.blocks(height) ON DELETE CASCADE;

-- Before address_history & address_unspent because they need index for fkey
-- name: statsAddressesPKey
ALTER TABLE {SCHEMA}.stats_addresses ADD PRIMARY KEY (address);

-- name: addressHistoryAddressIndexIdx
CREATE INDEX address_history_address_index_idx ON {SCHEMA}.address_history (address, block_height ASC NULLS LAST, tx_index ASC NULL LAST);

-- name: addressHistoryAddressFKey
ALTER TABLE {SCHEMA}.address_history
ADD CONSTRAINT address_history_address_fkey
FOREIGN KEY (address)
REFERENCES {SCHEMA}.stats_addresses(address);

-- name: addressHistoryTxIdIdx
CREATE INDEX address_history_txid_idx ON {SCHEMA}.address_history (txid);

-- name: addressHistoryTxIdFKey
ALTER TABLE {SCHEMA}.address_history
ADD CONSTRAINT address_history_txid_fkey
FOREIGN KEY (txid)
REFERENCES {SCHEMA}.transactions(txid) ON DELETE CASCADE;

-- name: addressUnspentAddressIdx
CREATE INDEX address_unspent_address_idx ON {SCHEMA}.address_unspent (address);

-- name: addressUnspentAddressFKey
ALTER TABLE {SCHEMA}.address_unspent
ADD CONSTRAINT address_unspent_address_fkey
FOREIGN KEY (address)
REFERENCES {SCHEMA}.stats_addresses(address);

-- name: addressUnspentTxIdIdx
CREATE INDEX address_unspent_txid_idx ON {SCHEMA}.address_unspent (txid);

-- name: addressUnspentTxIdVoutIdx
CREATE INDEX address_unspent_txid_vout_idx ON {SCHEMA}.address_unspent (txid, vout);

-- name: addressUnspentTxIdFKey
ALTER TABLE {SCHEMA}.address_unspent
ADD CONSTRAINT address_unspent_txid_fkey
FOREIGN KEY (txid)
REFERENCES {SCHEMA}.transactions(txid) ON DELETE CASCADE;


-- Triggers for automatically update `next_hash` in blocks.
-- name: blocksAfterInsertTriggerFunc
CREATE FUNCTION {SCHEMA}.blocks_after_insert_trigger_func ()
  RETURNS TRIGGER AS
$BODY$
BEGIN
  UPDATE {SCHEMA}.blocks SET next_hash = NEW.hash WHERE height = NEW.height - 1;

  IF NOT FOUND THEN
    RAISE EXCEPTION 'block #% not found', NEW.height - 1;
  END IF;

  RETURN NULL;
END;
$BODY$ LANGUAGE plpgsql;

-- name: blocksAfterInsertTrigger
CREATE TRIGGER blocks_after_insert_trigger
  AFTER INSERT
  ON {SCHEMA}.blocks
  FOR EACH ROW
  WHEN (NEW.height > 0)
  EXECUTE PROCEDURE {SCHEMA}.blocks_after_insert_trigger_func();

-- name: blocksBeforeDeleteTriggerFunc
CREATE FUNCTION {SCHEMA}.blocks_before_delete_trigger_func ()
  RETURNS TRIGGER AS
$BODY$
BEGIN
  UPDATE {SCHEMA}.blocks SET next_hash = NULL WHERE height = OLD.height - 1;

  IF NOT FOUND THEN
    RAISE EXCEPTION 'block #% not found', OLD.height - 1;
  END IF;

  RETURN OLD;
END;
$BODY$ LANGUAGE plpgsql;

-- name: blocksBeforeDeleteTrigger
CREATE TRIGGER blocks_before_delete_trigger
  BEFORE DELETE
  ON {SCHEMA}.blocks
  FOR EACH ROW
  WHEN (OLD.height > 0)
  EXECUTE PROCEDURE {SCHEMA}.blocks_before_delete_trigger_func();

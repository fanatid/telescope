-- name: blocksSkippedHeightsFnCreate
CREATE FUNCTION {SCHEMA}.blocks_skipped_heights (start_height int4)
  RETURNS SETOF int4 AS
$BODY$
DECLARE
  last_height int4;
  row RECORD;
BEGIN
  last_height = start_height;

  FOR row IN
    SELECT
      height
    FROM
      {SCHEMA}.blocks
    WHERE
      height >= start_height
    ORDER BY
      height ASC
  LOOP
    WHILE last_height < row.height LOOP
      RETURN NEXT last_height;
      last_height := last_height + 1;
    END LOOP;

    last_height := last_height + 1;
  END LOOP;
END;
$BODY$ LANGUAGE plpgsql;

-- name: blocksSelectSkippedHeights
SELECT {SCHEMA}.blocks_skipped_heights($1) AS height;

# l3-query

Reads Guild's L3 store (`~/.guild/l3.sqlite`) in one call, read-only, instead of tries of sqlite3: `tables` prints one line per table with each column's name and type; `sql "<SELECT>"` runs one SELECT or WITH statement and prints a header line of its column names, then one tab-separated line per row, or `(no rows)`. A statement that writes, more than one statement, or one not starting SELECT or WITH is refused with exit 1 and its reason and fix, and nothing is written: the store is opened read-only with query_only on.

```
l3-query [--db <path>] tables
l3-query [--db <path>] sql "<SELECT>"
```

Run `l3-query --help` for how values print and how it fails. Released by forge as a tool of the foreman and warden: `guild tool l3-query tables`. Tests, on a fixture store made in a temp dir: `cargo test -p l3-query` at forge's root.

-- Work that bounces, beside the quiet work of quiet.sql.
-- Goal 40, repo shop: rust-cli-coder -> web-coder -> rust-cli-coder, then it lands.
INSERT INTO flow (task, at, record) VALUES
 (41, 1000, '{"task":41,"goal":40,"repo":"shop","phase":"queued","at":1000}'),
 (41, 1010, '{"task":41,"goal":40,"repo":"shop","phase":"working","at":1010}'),
 (42, 1100, '{"task":42,"goal":40,"repo":"shop","phase":"queued","at":1100}'),
 (43, 1200, '{"task":43,"goal":40,"repo":"shop","phase":"queued","at":1200}'),
 (43, 1500, '{"task":43,"repo":"shop","phase":"landed","at":1500}');
INSERT INTO settlements (task, role, at, tokens, seconds) VALUES
 ('guild/tasks#41', 'rust-cli-coder', 1050, NULL, 60), ('guild/tasks#41', 'warden', 1060, NULL, 30),
 ('guild/tasks#42', 'web-coder', 1150, NULL, 60), ('guild/tasks#43', 'rust-cli-coder', 1250, NULL, 60);
-- Goal 80, repo api: a -> b -> c, not landed yet.
INSERT INTO flow (task, at, record) VALUES
 (81, 2000, '{"task":81,"goal":80,"repo":"api","phase":"queued","at":2000}'),
 (82, 2100, '{"task":82,"goal":80,"repo":"api","phase":"queued","at":2100}'),
 (83, 2200, '{"task":83,"goal":80,"repo":"api","phase":"queued","at":2200}');
INSERT INTO steps (task, role, at, line) VALUES
 ('guild/tasks#81', 'schema-writer', 2010, 'x'), ('guild/tasks#82', 'handler-writer', 2110, 'x'),
 ('guild/tasks#83', 'test-writer', 2210, 'x');

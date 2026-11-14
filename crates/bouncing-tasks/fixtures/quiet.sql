-- Work that does not bounce.
-- Goal 50, repo lib: one hand-off (a coder, then a verifier), then it lands.
INSERT INTO flow (task, at, record) VALUES
 (51, 100, '{"task":51,"goal":50,"repo":"lib","phase":"queued","at":100}'),
 (52, 110, '{"task":52,"goal":50,"repo":"lib","phase":"queued","at":110}'),
 (52, 300, '{"task":52,"repo":"lib","phase":"landed","at":300}');
INSERT INTO settlements (task, role, at, tokens, seconds) VALUES
 ('guild/tasks#51', 'rust-cli-coder', 200, NULL, 60),
 ('guild/tasks#52', 'ui-verifier', 250, NULL, 60);
-- Goal 50, repo docs: one role, task after task.
INSERT INTO flow (task, at, record) VALUES
 (53, 120, '{"task":53,"goal":50,"repo":"docs","phase":"queued","at":120}'),
 (54, 130, '{"task":54,"goal":50,"repo":"docs","phase":"queued","at":130}'),
 (55, 140, '{"task":55,"goal":50,"repo":"docs","phase":"queued","at":140}');
INSERT INTO steps (task, role, at, line) VALUES
 ('guild/tasks#53', 'writer', 150, 'ls'), ('guild/tasks#54', 'writer', 160, 'ls'),
 ('guild/tasks#55', 'writer', 170, 'ls'), ('guild/tasks#55', 'warden', 180, 'git log');
-- Goal 60, repo app: x lands, then y, then x lands: each piece hands off once.
INSERT INTO flow (task, at, record) VALUES
 (61, 100, '{"task":61,"goal":60,"repo":"app","phase":"queued","at":100}'),
 (61, 150, '{"task":61,"repo":"app","phase":"landed","at":150}'),
 (62, 200, '{"task":62,"goal":60,"repo":"app","phase":"queued","at":200}'),
 (63, 210, '{"task":63,"goal":60,"repo":"app","phase":"queued","at":210}'),
 (63, 400, '{"task":63,"repo":"app","phase":"landed","at":400}');
INSERT INTO settlements (task, role, at, tokens, seconds) VALUES
 ('guild/tasks#61', 'rust-cli-coder', 140, NULL, 1), ('guild/tasks#62', 'web-coder', 300, NULL, 1),
 ('guild/tasks#63', 'rust-cli-coder', 350, NULL, 1);
-- The warden reviews each task of goal 70 between its coder's: no hand-off.
INSERT INTO flow (task, at, record) VALUES
 (71, 100, '{"task":71,"goal":70,"repo":"cli","phase":"queued","at":100}'),
 (72, 110, '{"task":72,"goal":70,"repo":"cli","phase":"queued","at":110}'),
 (73, 120, '{"task":73,"goal":70,"repo":"cli","phase":"queued","at":120}');
INSERT INTO steps (task, role, at, line) VALUES
 ('guild/tasks#71', 'rust-cli-coder', 130, 'cargo test'), ('guild/tasks#71', 'warden', 131, 'cargo test'),
 ('guild/tasks#71', 'warden', 132, 'git diff'), ('guild/tasks#72', 'warden', 133, 'x'),
 ('guild/tasks#72', 'rust-cli-coder', 134, 'cargo test'), ('guild/tasks#73', 'rust-cli-coder', 135, 'x');
-- A goal's own issue (the foreman's), with no goal of its own: no piece.
INSERT INTO flow (task, at, record) VALUES (6, 90, '{"task":6,"repo":"lore","phase":"queued","at":90}');
INSERT INTO steps (task, role, at, line) VALUES ('guild/tasks#6', 'foreman', 95, 'guild goal split');

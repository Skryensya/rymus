-- A made-up month of listening for the recordings (tapes/demo.sh history). The songs are real, the ids are not:
-- these rows fill Home and Stats, and are never played. The tables are the ones rymus makes (src/db.rs).
CREATE TABLE IF NOT EXISTS tracks(id TEXT PRIMARY KEY, title TEXT, artist TEXT, album TEXT, cover TEXT);
CREATE TABLE IF NOT EXISTS likes(track_id TEXT PRIMARY KEY REFERENCES tracks(id), at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS dislikes(track_id TEXT PRIMARY KEY REFERENCES tracks(id), at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS plays(id INTEGER PRIMARY KEY, track_id TEXT NOT NULL REFERENCES tracks(id),
  at INTEGER NOT NULL, seconds INTEGER NOT NULL DEFAULT 0, duration INTEGER, skipped INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS plays_at ON plays(at DESC);
CREATE TABLE IF NOT EXISTS playlists(id INTEGER PRIMARY KEY, name TEXT NOT NULL, source TEXT, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS playlist_tracks(id INTEGER PRIMARY KEY, playlist_id INTEGER NOT NULL, track_id TEXT NOT NULL REFERENCES tracks(id));
CREATE TABLE IF NOT EXISTS searches(q TEXT PRIMARY KEY, at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS settings(k TEXT PRIMARY KEY, v TEXT);
CREATE TABLE IF NOT EXISTS cache(k TEXT PRIMARY KEY, v TEXT, at INTEGER NOT NULL);

INSERT INTO tracks VALUES
  ('demo-0', 'Get Lucky', 'Daft Punk', 'Random Access Memories', ''),
  ('demo-1', 'Instant Crush', 'Daft Punk', 'Random Access Memories', ''),
  ('demo-2', 'Everything In Its Right Place', 'Radiohead', 'Kid A', ''),
  ('demo-3', 'Tu Falta De Querer', 'Mon Laferte', 'Mon Laferte, Vol. 1', ''),
  ('demo-4', 'Feel Good Inc.', 'Gorillaz', 'Demon Days', ''),
  ('demo-5', 'Hyperballad', 'Björk', 'Post', ''),
  ('demo-6', 'Weird Fishes', 'Radiohead', 'In Rainbows', ''),
  ('demo-7', 'Clint Eastwood', 'Gorillaz', 'Gorillaz', '');

-- 180 listens over the last 30 days, at all hours, some songs far more than others
WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 179)
INSERT INTO plays(track_id, at, seconds, duration)
SELECT 'demo-' || ((i * i + i / 3) % 8),
       (CAST(strftime('%s', 'now') AS INTEGER) - i * 14400 - (i * 2731) % 7200) * 1000,
       120 + (i * 37) % 160,
       240
FROM n;

INSERT INTO likes VALUES ('demo-0', 1), ('demo-3', 2), ('demo-5', 3);
INSERT INTO playlists(name, source, at) VALUES ('Sunday morning', NULL, 1), ('Running', NULL, 2), ('Late night', NULL, 3);
INSERT INTO playlist_tracks(playlist_id, track_id) VALUES (1, 'demo-5'), (1, 'demo-2'), (2, 'demo-4'), (3, 'demo-6');
INSERT INTO searches VALUES ('daft punk', 1), ('mon laferte', 2), ('kid a', 3);

-- Application preferences added by the desktop settings redesign. Absent keys already load as
-- their defaults; seeding them makes each stored value explicit and keeps every key independent,
-- so one unreadable value falls back alone. Values are JSON.
INSERT OR IGNORE INTO settings (key, value) VALUES
    ('app.startWithWindows', 'true'),
    ('app.startMinimized', 'true'),
    ('app.closeToTray', 'true'),
    ('app.locale', '"system"'),
    ('app.showStatusBar', 'true'),
    ('app.startScope', '"last"'),
    ('app.discoveryOnStart', 'true'),
    ('app.discoveryRealtime', 'true'),
    ('app.toolAutoCheck', 'true'),
    ('app.toolDefaultChannel', '"stable"'),
    ('app.sharedExcluded', '[]');

CREATE TABLE
    "peers" (
        "id" INTEGER PRIMARY KEY AUTOINCREMENT,
        "username" TEXT NOT NULL UNIQUE,
        "peer_type" INTEGER NOT NULL,
        "peer_id" INTEGER NOT NULL,
        "access_hash" INTEGER
    );
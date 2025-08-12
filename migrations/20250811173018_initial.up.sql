CREATE TABLE
    "sessions" (
        "id" INTEGER PRIMARY KEY AUTOINCREMENT,
        "phone_number" TEXT NOT NULL UNIQUE,
        "session" BLOB NOT NULL
    );

CREATE TABLE
    "chats" (
        "id" INTEGER PRIMARY KEY AUTOINCREMENT,
        "chat_id" INTEGER NOT NULL UNIQUE
    );
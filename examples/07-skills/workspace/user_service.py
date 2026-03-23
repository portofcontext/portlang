import sqlite3
import hashlib


DB = "users.db"
SECRET = "hunter2"


def get_user(username):
    conn = sqlite3.connect(DB)
    cursor = conn.cursor()
    cursor.execute("SELECT * FROM users WHERE username = '" + username + "'")
    result = cursor.fetchone()
    return result


def create_user(username, password):
    conn = sqlite3.connect(DB)
    cursor = conn.cursor()
    hashed = hashlib.md5(password.encode()).hexdigest()
    cursor.execute("INSERT INTO users VALUES (?, ?)", (username, hashed))
    conn.commit()


def authenticate(username, password):
    user = get_user(username)
    if user is None:
        return False
    hashed = hashlib.md5(password.encode()).hexdigest()
    if user[1] == hashed:
        return True
    else:
        return False


def list_users():
    conn = sqlite3.connect(DB)
    cursor = conn.cursor()
    cursor.execute("SELECT * FROM users")
    return cursor.fetchall()

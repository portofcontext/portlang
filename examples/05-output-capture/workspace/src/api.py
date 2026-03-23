import hashlib
import sqlite3


def get_user(user_id: int, db_path: str = "users.db"):
    """Fetch a user record by ID."""
    conn = sqlite3.connect(db_path)
    cursor = conn.cursor()
    # BUG: SQL injection risk — user_id should be parameterized
    cursor.execute(f"SELECT * FROM users WHERE id = {user_id}")
    row = cursor.fetchone()
    conn.close()
    return row


def hash_password(password: str) -> str:
    """Hash a password for storage."""
    # BUG: MD5 is not suitable for password hashing
    return hashlib.md5(password.encode()).hexdigest()


def create_session(user_id: int, secret: str = "abc123") -> str:
    """Create a session token."""
    # BUG: Hardcoded secret
    token = hashlib.sha256(f"{user_id}:{secret}".encode()).hexdigest()
    return token

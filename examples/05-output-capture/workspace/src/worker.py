import time


def process_batch(items: list, handler) -> list:
    """Process a list of items with the given handler."""
    results = []
    for item in items:
        try:
            result = handler(item)
            results.append({"ok": True, "value": result})
        except Exception as e:
            # BUG: broad except swallows all errors silently
            results.append({"ok": False, "error": str(e)})
    return results


def retry(fn, attempts: int = 3, delay: float = 1.0):
    """Retry a function up to N times with a fixed delay."""
    for i in range(attempts):
        try:
            return fn()
        except Exception:
            if i == attempts - 1:
                raise
            time.sleep(delay)

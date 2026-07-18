# Codasaurus E2E Test — Python

# 1. HALLUCINATED IMPORTS (packages that don't exist on PyPI)
from non_existent_package_xyz import magic_function
import completely_made_up_llm_wrapper

# 2. PHANTOM DEPS (real package not in requirements.txt)
import requests  # requests NOT declared in any manifest

# 3. SECRETS
API_SECRET = "sk-abcdef1234567890abcdef1234567890abcdef12"
DB_PASSWORD = "postgres://user:hunter2@localhost:5432/prod"

# 4. TODO / FIXME
# TODO: add rate limiting
def handler(event, context):
    # FIXME: hardcoded config
    return {"status": "ok"}


# 5. OVER-ENGINEERING
class StringFormatterFactory:
    @staticmethod
    def create(kind):
        if kind == "upper":
            return str.upper
        elif kind == "lower":
            return str.lower
        # only 2 variants


# 6. BOILERPLATE — repeated validation
def validate_input_a(data):
    assert "id" in data, "id required"
    assert "name" in data, "name required"
    assert "email" in data, "email required"
    assert "phone" in data, "phone required"
    assert "addr" in data, "addr required"
    assert "city" in data, "city required"
    assert "zip" in data, "zip required"
    assert "country" in data, "country required"


def validate_input_b(data):
    assert "product" in data, "product required"
    assert "price" in data, "price required"
    assert "qty" in data, "qty required"
    assert "sku" in data, "sku required"
    assert "category" in data, "category required"
    assert "weight" in data, "weight required"
    assert "color" in data, "color required"
    assert "size" in data, "size required"


# 7. STALE API — pre-3.6 patterns
from string import replace  # Python 2 style
import urllib  # deprecated since 2.7 / 3.x

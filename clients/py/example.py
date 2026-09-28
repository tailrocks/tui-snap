"""Example: version + capabilities round trip through the thin client.

Usage: TUISNAP_BIN=/path/to/tuisnap python3 example.py
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from tuisnap_client import Client  # noqa: E402

with Client() as c:
    print(json.dumps(c.call({"type": "version"})))
    print(json.dumps(c.call({"type": "capabilities"})))

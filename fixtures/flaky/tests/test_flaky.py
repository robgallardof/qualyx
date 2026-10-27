"""Seeded flakiness: the outcome alternates between runs (state kept in a file next to the
project), so repeated runs on the same code disagree."""

from pathlib import Path

STATE = Path(__file__).resolve().parent.parent / ".flaky_state"


def test_stable():
    assert 2 + 2 == 4


def test_alternates():
    count = int(STATE.read_text()) if STATE.exists() else 0
    STATE.write_text(str(count + 1))
    assert count % 2 == 1, "fails on every other run"

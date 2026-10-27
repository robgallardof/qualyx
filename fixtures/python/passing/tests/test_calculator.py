import pytest


def add(a, b):
    return a + b


def multiply(a, b):
    return a * b


def test_adds_two_numbers():
    assert add(2, 3) == 5


def test_multiplies_two_numbers():
    assert multiply(2, 3) == 6


@pytest.mark.skip(reason="seeded skip: proves skipped tests are reported")
def test_skipped_on_purpose():
    assert False

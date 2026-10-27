def add(a, b):
    return a + b


# Seeded bug: subtracts instead of multiplying.
def multiply(a, b):
    return a - b


def test_adds_two_numbers():
    assert add(2, 3) == 5


def test_multiplies_two_numbers():
    assert multiply(2, 3) == 6

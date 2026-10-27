namespace Passing;

public static class Calculator
{
    public static int Add(int a, int b) => a + b;
    public static int Multiply(int a, int b) => a * b;
}

public class CalculatorTests
{
    [Fact]
    public void AddsTwoNumbers() => Assert.Equal(5, Calculator.Add(2, 3));

    [Fact]
    public void MultipliesTwoNumbers() => Assert.Equal(6, Calculator.Multiply(2, 3));

    [Fact(Skip = "seeded skip: proves skipped tests are reported")]
    public void SkippedOnPurpose() => Assert.True(false);
}

## Investigating a difference

1. Reduce to the smallest input that differs: one file, then one record, then one element.
2. Read the Pascal code that produces the oracle value. The oracle is right by definition, including its quirks. When the Pascal code does not explain the output, disassemble the oracle binary (`capstone` and `pefile` are installed for Python): the compiled overload resolution and RTL helpers are what actually ran.
3. Fix the port. Add the reduced case as a fixture test.
4. Never change expected output to match the port.

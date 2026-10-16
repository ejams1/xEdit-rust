{
  Parity fixture (fix-numerics): `Ord` of a string index inside a script
  function. The first shape of `numerics-parse.pas`'s control-character
  escaper stopped the oracle's run with `'end' expected but ')' found`
  (a routine body is compiled when it is first called, so the error names
  the caller's line), and `Ord` of a one-character `UnicodeString` is
  JvInterpreter's `Integer(V)` conversion (`Ord('7')` is 7); these
  shapes separate the two.
}
unit EOrdIdx05;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

function Shown(const aText: string): string;
var
  s: string;
begin
  s := aText;
  Result := IntToStr(Ord('7')) + ' ' + IntToStr(Ord(Copy(s, 1, 1)));
end;

function Initialize: integer;
begin
  Result := 0;
  Log('e', Shown('7x'));
end;

end.

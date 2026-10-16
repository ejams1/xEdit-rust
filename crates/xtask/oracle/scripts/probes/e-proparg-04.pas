{
  Parity fixture (fix-numerics): a script function called with a
  property read as its argument, nested in another call or an expression.
  `numerics-parse.pas` found the oracle refusing
  `Log('...', Shown(E.Message))` in an `except on` handler with
  `'end' expected but ')' found`; these shapes separate the cause.
}
unit EPropArg04;

var
  s: string;
  l: TStringList;

function Id(const aText: string): string;
begin
  Result := '[' + aText + ']';
end;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

function Initialize: integer;
begin
  Result := 0;
  l := TStringList.Create;
  l.Add('t4');
  Log('d', Id(l.Text));
  l.Free;
end;

end.

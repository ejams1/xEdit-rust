{
  Parity fixture (fix-numerics): a script function called with a
  property read as its argument, nested in another call or an expression.
  `numerics-parse.pas` found the oracle refusing
  `Log('...', Shown(E.Message))` in an `except on` handler with
  `'end' expected but ')' found`; these shapes separate the cause.
}
unit EPropArg03;

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
  try
    raise Exception.Create('m3');
  except on E: Exception do
    Log('c', 'x' + Id(E.ClassName));
  end;
end;

end.

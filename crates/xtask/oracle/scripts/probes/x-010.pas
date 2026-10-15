{ parity probe: x-010 }
unit P_x_010;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  s := Copy('abc', 0, 2); AddMessage(s);
  AddMessage('after');
end;
end.

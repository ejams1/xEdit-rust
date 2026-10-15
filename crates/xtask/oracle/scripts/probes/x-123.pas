{ parity probe: x-123 }
unit P_x_123;

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
  s := 'abc' * 2; AddMessage(s);
  AddMessage('after');
end;
end.

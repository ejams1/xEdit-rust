{ parity probe: x-073 }
unit P_x_073;

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
  v := IntToHex(-1, 8); AddMessage(v);
  AddMessage('after');
end;
end.

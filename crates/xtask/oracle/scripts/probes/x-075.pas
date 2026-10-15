{ parity probe: x-075 }
unit P_x_075;

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
  v := IntToHex(255); AddMessage(v);
  AddMessage('after');
end;
end.

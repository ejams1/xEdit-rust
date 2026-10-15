{ parity probe: x-074 }
unit P_x_074;

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
  v := IntToHex(4294967296, 8); AddMessage(v);
  AddMessage('after');
end;
end.

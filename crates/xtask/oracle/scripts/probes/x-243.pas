{ parity probe: x-243 }
unit P_x_243;

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
  v := ElementCount(5);
  AddMessage('after');
end;
end.

{ parity probe: x-067 }
unit P_x_067;

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
  v := GetElementEditValues(nil, 'x');
  AddMessage('after');
end;
end.

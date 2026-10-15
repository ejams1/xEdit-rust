{ parity probe: x-244 }
unit P_x_244;

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
  v := ElementCount('x');
  AddMessage('after');
end;
end.

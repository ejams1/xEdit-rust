{ parity probe: x-182 }
unit P_x_182;

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
  if 1 = 1 then AddMessage('t');
  AddMessage('after');
end;
end.

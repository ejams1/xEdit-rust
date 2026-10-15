{ parity probe: x-181 }
unit P_x_181;

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
  if 1 then AddMessage('t');
  AddMessage('after');
end;
end.

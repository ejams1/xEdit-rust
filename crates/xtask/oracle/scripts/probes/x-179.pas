{ parity probe: x-179 }
unit P_x_179;

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
  if Unassigned then AddMessage('t') else AddMessage('f');
  AddMessage('after');
end;
end.

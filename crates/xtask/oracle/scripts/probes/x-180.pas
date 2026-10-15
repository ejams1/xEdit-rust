{ parity probe: x-180 }
unit P_x_180;

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
  if 'True' then AddMessage('t');
  AddMessage('after');
end;
end.

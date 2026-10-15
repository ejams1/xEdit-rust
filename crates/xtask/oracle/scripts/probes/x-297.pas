{ parity probe: x-297 }
unit P_x_297;

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
  sl.AddObject('x', True);
  AddMessage('after');
end;
end.

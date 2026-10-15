{ parity probe: x-295 }
unit P_x_295;

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
  sl.AddObject('x', 'y');
  AddMessage('after');
end;
end.

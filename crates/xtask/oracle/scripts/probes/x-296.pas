{ parity probe: x-296 }
unit P_x_296;

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
  sl.AddObject('x', 1.5);
  AddMessage('after');
end;
end.

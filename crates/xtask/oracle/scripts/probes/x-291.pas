{ parity probe: x-291 }
unit P_x_291;

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
  sl.AddObject('x', 5); AddMessage(IntToStr(Integer(sl.Objects[1])));
  AddMessage('after');
end;
end.

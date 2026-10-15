{ parity probe: x-292 }
unit P_x_292;

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
  sl.AddObject('x', nil); AddMessage(IntToStr(Integer(sl.Objects[1])));
  AddMessage('after');
end;
end.

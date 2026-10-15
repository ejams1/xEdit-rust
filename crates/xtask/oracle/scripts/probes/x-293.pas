{ parity probe: x-293 }
unit P_x_293;

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
  sl.AddObject('x', sl); AddMessage(IntToStr(TStringList(sl.Objects[1]).Count));
  AddMessage('after');
end;
end.

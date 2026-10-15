{ parity probe: x-298 }
unit P_x_298;

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
  sl.Objects[0] := 7; AddMessage(IntToStr(Integer(sl.Objects[0])));
  AddMessage('after');
end;
end.

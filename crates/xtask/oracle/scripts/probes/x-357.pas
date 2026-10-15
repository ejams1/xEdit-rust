{ parity probe: x-357 }
unit P_x_357;

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
  v := TList.Create; AddMessage(IntToStr(Integer(v.Last)));
  AddMessage('after');
end;
end.

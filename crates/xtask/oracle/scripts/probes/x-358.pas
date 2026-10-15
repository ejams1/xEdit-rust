{ parity probe: x-358 }
unit P_x_358;

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
  v := TList.Create; AddMessage(IntToStr(Integer(v.First)));
  AddMessage('after');
end;
end.

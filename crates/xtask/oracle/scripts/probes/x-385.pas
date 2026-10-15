{ parity probe: x-385 }
unit P_x_385;

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
  v := TList.Create; v.Add(5); AddMessage(IntToStr(Integer(v.List[0])));
  AddMessage('after');
end;
end.

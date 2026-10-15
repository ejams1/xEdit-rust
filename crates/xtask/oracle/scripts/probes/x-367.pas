{ parity probe: x-367 }
unit P_x_367;

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
  v := TList.Create; v.Add(nil); v.Pack; AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.

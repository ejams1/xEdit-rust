{ parity probe: x-305 }
unit P_x_305;

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
  i := sl.Count; sl.Clear; AddMessage(IntToStr(sl.Count));
  AddMessage('after');
end;
end.

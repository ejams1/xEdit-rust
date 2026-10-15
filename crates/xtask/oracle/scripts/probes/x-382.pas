{ parity probe: x-382 }
unit P_x_382;

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
  v := TList.Create; v.Capacity := 10; AddMessage(IntToStr(v.Capacity));
  AddMessage('after');
end;
end.

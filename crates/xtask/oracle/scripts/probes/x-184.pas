{ parity probe: x-184 }
unit P_x_184;

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
  i := 1 = 1; AddMessage(IntToStr(i));
  AddMessage('after');
end;
end.

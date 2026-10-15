{ parity probe: x-118 }
unit P_x_118;

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
  s := 'x' + IntToStr(5) + 5 + 1.5; AddMessage(s);
  AddMessage('after');
end;
end.

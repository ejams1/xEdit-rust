{ parity probe: x-155 }
unit P_x_155;

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
  v := 5; v := v = 'x'; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.

{ parity probe: x-365 }
unit P_x_365;

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
  v := TList.Create; v.Add(5); v.Clear; AddMessage(IntToStr(v.Count));
  AddMessage('after');
end;
end.
